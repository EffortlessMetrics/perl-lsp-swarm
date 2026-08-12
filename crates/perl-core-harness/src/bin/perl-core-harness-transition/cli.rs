#[derive(Debug, Default)]
struct Options {
    values: BTreeMap<String, VecDeque<String>>,
}

impl Options {
    fn parse(args: impl Iterator<Item = String>) -> Result<Self> {
        let mut args = args;
        let mut values = BTreeMap::<String, VecDeque<String>>::new();
        while let Some(flag) = args.next() {
            if !flag.starts_with("--") {
                bail!("expected an option beginning with --, found {flag}");
            }
            let value = args
                .next()
                .ok_or_else(|| color_eyre::eyre::eyre!("missing value for {flag}"))?;
            if value.starts_with("--") {
                bail!("missing value for {flag}; found option {value}");
            }
            values.entry(flag).or_default().push_back(value);
        }
        Ok(Self { values })
    }

    fn required(&mut self, flag: &str) -> Result<String> {
        let value = self
            .values
            .get_mut(flag)
            .and_then(VecDeque::pop_front)
            .ok_or_else(|| color_eyre::eyre::eyre!("required option {flag} was not supplied"))?;
        if self
            .values
            .get(flag)
            .is_some_and(|values| !values.is_empty())
        {
            bail!("option {flag} may be supplied only once");
        }
        self.values.remove(flag);
        Ok(value)
    }

    fn finish(self) -> Result<()> {
        if self.values.is_empty() {
            return Ok(());
        }
        bail!(
            "unrecognized option(s): {}",
            self.values.keys().cloned().collect::<Vec<_>>().join(", ")
        )
    }
}

#[derive(Debug)]
struct ClassifyConfig {
    accepted_baseline: PathBuf,
    compile: PathBuf,
    output: PathBuf,
}

impl ClassifyConfig {
    fn from_options(mut options: Options) -> Result<Self> {
        let config = Self {
            accepted_baseline: PathBuf::from(options.required("--accepted-baseline")?),
            compile: PathBuf::from(options.required("--compile")?),
            output: PathBuf::from(options.required("--output")?),
        };
        options.finish()?;
        Ok(config)
    }
}

#[cfg(test)]
mod options_finish_observer {
    use super::*;

    /// RIPR-named observer for Options::finish unrecognized-option bail.
    #[test]
    fn unrecognized_option_finish_bail_is_observed() {
        let options = Options::parse(
            ["--series".to_string(), "series.json".to_string()].into_iter(),
        )
        .expect("parse options");
        let err = options
            .finish()
            .expect_err("unrecognized options must fail")
            .to_string();
        assert_eq!(err, "unrecognized option(s): --series");
    }

    #[test]
    fn duplicate_option_is_rejected() {
        let err = Options::parse(
            [
                "--output".to_string(),
                "a.json".to_string(),
                "--output".to_string(),
                "b.json".to_string(),
            ]
            .into_iter(),
        )
        .and_then(|mut options| options.required("--output").map(|_| ()))
        .expect_err("duplicate option must fail")
        .to_string();
        assert_eq!(err, "option --output may be supplied only once");
    }
}
