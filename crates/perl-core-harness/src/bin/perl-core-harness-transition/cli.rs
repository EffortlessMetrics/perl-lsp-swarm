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
            match flag.as_str() {
                "--accepted-baseline" | "--compile" | "--output" | "--receipt" | "--series" => {}
                _ => bail!("unrecognized option(s): {flag}"),
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

    fn optional(&mut self, flag: &str) -> Result<Option<String>> {
        let Some(values) = self.values.get_mut(flag) else {
            return Ok(None);
        };
        // `Options::parse` only records a flag after reading its value, so an
        // empty queue here is a defensive no-op (absent), not a distinct error.
        let Some(value) = values.pop_front() else {
            self.values.remove(flag);
            return Ok(None);
        };
        if !values.is_empty() {
            bail!("option {flag} may be supplied only once");
        }
        self.values.remove(flag);
        Ok(Some(value))
    }

    fn reject_unused(self) -> Result<()> {
        if self.values.is_empty() {
            return Ok(());
        }
        let unused = self.values.keys().cloned().collect::<Vec<_>>().join(", ");
        bail!("unrecognized option(s) for command: {unused}");
    }
}

#[derive(Debug)]
struct ClassifyConfig {
    accepted_baseline: PathBuf,
    compile: PathBuf,
    output: PathBuf,
    series: Option<PathBuf>,
}

impl ClassifyConfig {
    fn from_options(mut options: Options) -> Result<Self> {
        let config = Self {
            accepted_baseline: PathBuf::from(options.required("--accepted-baseline")?),
            compile: PathBuf::from(options.required("--compile")?),
            output: PathBuf::from(options.required("--output")?),
            series: options.optional("--series")?.map(PathBuf::from),
        };
        options.reject_unused()?;
        Ok(config)
    }
}

#[derive(Debug)]
struct CheckConfig {
    accepted_baseline: PathBuf,
    compile: PathBuf,
    receipt: PathBuf,
    series: Option<PathBuf>,
}

impl CheckConfig {
    fn from_options(mut options: Options) -> Result<Self> {
        let config = Self {
            accepted_baseline: PathBuf::from(options.required("--accepted-baseline")?),
            compile: PathBuf::from(options.required("--compile")?),
            receipt: PathBuf::from(options.required("--receipt")?),
            series: options.optional("--series")?.map(PathBuf::from),
        };
        options.reject_unused()?;
        Ok(config)
    }
}
