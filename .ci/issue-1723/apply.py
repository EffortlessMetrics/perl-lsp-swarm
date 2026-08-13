from pathlib import Path

path = Path("crates/perl-semantic-analyzer/src/analysis/type_inference.rs")
source = path.read_text()


def replace_once(old: str, new: str) -> None:
    global source
    if source.count(old) != 1:
        raise SystemExit(f"expected one source marker, found {source.count(old)}: {old[:80]!r}")
    source = source.replace(old, new, 1)


replace_once(
    '''    /// Narrow source-backed method return facts keyed by `(package, method)`.
    method_return_facts: HashMap<(String, String), TypeFact>,
    /// Type aliases from use statements
''',
    '''    /// Narrow source-backed method return facts keyed by `(package, method)`.
    method_return_facts: HashMap<(String, String), TypeFact>,
    /// Explicit result types collected for each active subroutine inference frame.
    return_type_stack: Vec<Vec<PerlType>>,
    /// Type aliases from use statements
''',
)
replace_once(
    '''            accessor_return_facts: HashMap::new(),
            method_return_facts: HashMap::new(),
            _type_aliases: HashMap::new(),
''',
    '''            accessor_return_facts: HashMap::new(),
            method_return_facts: HashMap::new(),
            return_type_stack: Vec::new(),
            _type_aliases: HashMap::new(),
''',
)
replace_once(
    '''    pub fn infer(&mut self, ast: &Node) -> Result<PerlType, Vec<TypeConstraint>> {
        self.refresh_accessor_return_facts(ast);
''',
    '''    pub fn infer(&mut self, ast: &Node) -> Result<PerlType, Vec<TypeConstraint>> {
        self.return_type_stack.clear();
        self.refresh_accessor_return_facts(ast);
''',
)

subroutine_start = source.index("            NodeKind::Subroutine { name, body, .. } => {")
subroutine_tail = source.index(
    "                let sub_type = Subroutine { params: param_types, returns: vec![return_type] };",
    subroutine_start,
)
source = (
    source[:subroutine_start]
    + '''            NodeKind::Subroutine { name, body, .. } => {
                // Create new scope for subroutine
                let mut sub_env = TypeEnvironment::with_parent(env.clone());

                // Default to accepting any parameters for now
                let param_types = vec![Any];

                // Collect explicit results during the existing sequential inference pass. A frame per
                // active subroutine keeps nested subroutine results isolated while preserving the
                // environment visible at each result site.
                self.return_type_stack.push(Vec::new());
                let implicit_return = self.infer_node(body, &mut sub_env);
                let mut return_types = self.return_type_stack.pop().unwrap_or_default();
                return_types.push(implicit_return?);
                let return_type = Self::unify_return_types(&return_types);

'''
    + source[subroutine_tail:]
)

replace_once(
    '''            NodeKind::Return { value } => {
                if let Some(val) = value {
                    self.infer_node(val, env)
                } else {
                    Ok(Void)
                }
            }
''',
    '''            NodeKind::StatementModifier { statement, condition, .. } => {
                let _condition_type = self.infer_node(condition, env)?;
                self.infer_node(statement, env)
            }

            NodeKind::Return { value } => {
                let return_type = if let Some(val) = value {
                    self.infer_node(val, env)?
                } else {
                    Void
                };

                if let Some(return_types) = self.return_type_stack.last_mut() {
                    return_types.push(return_type.clone());
                }

                Ok(return_type)
            }
''',
)

replace_once(
    '''    /// Check if two types are compatible
    fn types_compatible(&self, t1: &PerlType, t2: &PerlType) -> bool {
''',
    '''    /// Unify explicit and implicit subroutine result types without applying Perl's
    /// broad scalar-coercion compatibility rules. Distinct result shapes remain
    /// visible to downstream hover and completion consumers.
    fn unify_return_types(types: &[PerlType]) -> PerlType {
        use PerlType::*;
        use ScalarType::*;

        fn push_unique(flattened: &mut Vec<PerlType>, ty: &PerlType) {
            if let PerlType::Union(members) = ty {
                for member in members {
                    push_unique(flattened, member);
                }
            } else if !flattened.contains(ty) {
                flattened.push(ty.clone());
            }
        }

        let mut flattened = Vec::new();
        for ty in types {
            push_unique(&mut flattened, ty);
        }

        if flattened.iter().any(|ty| matches!(ty, Any)) {
            return Any;
        }

        if flattened.is_empty() {
            return Void;
        }

        if flattened.len() == 1 {
            return flattened.pop().unwrap_or(Void);
        }

        if flattened.iter().all(|ty| matches!(ty, Scalar(Integer) | Scalar(Float))) {
            if flattened.iter().any(|ty| matches!(ty, Scalar(Float))) {
                return Scalar(Float);
            }
            return Scalar(Integer);
        }

        if flattened.iter().all(|ty| matches!(ty, Scalar(_)))
            && flattened.iter().any(|ty| matches!(ty, Scalar(Mixed)))
        {
            return Scalar(Mixed);
        }

        if flattened.len() <= 3 { Union(flattened) } else { Any }
    }

    /// Check if two types are compatible
    fn types_compatible(&self, t1: &PerlType, t2: &PerlType) -> bool {
''',
)

path.write_text(source)
