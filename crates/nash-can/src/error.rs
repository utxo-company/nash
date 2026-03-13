use nash_ast::ModuleName;
use nash_region::Region;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadArityContext {
    TypeArity,
    PatternArity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
    BadOp,
    BadVar,
    BadPattern,
    BadType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error<'a> {
    MissingModuleHeader,
    NotFoundType {
        region: Region,
        prefix: Option<&'a str>,
        name: &'a str,
    },
    ImportNotFound {
        region: Region,
        module: &'a str,
    },
    AmbiguousType {
        region: Region,
        prefix: Option<&'a str>,
        name: &'a str,
        first_module: ModuleName<'a>,
        other_modules: &'a [ModuleName<'a>],
    },
    BadArity {
        region: Region,
        context: BadArityContext,
        name: &'a str,
        expected: usize,
        actual: usize,
    },
    ExportNotFound {
        region: Region,
        kind: VarKind,
        name: &'a str,
    },
    ExportOpenAlias {
        region: Region,
        name: &'a str,
    },
}
