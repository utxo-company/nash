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
    DuplicateDecl {
        name: &'a str,
        first: Region,
        second: Region,
    },
    DuplicateType {
        name: &'a str,
        first: Region,
        second: Region,
    },
    DuplicateCtor {
        name: &'a str,
        first: Region,
        second: Region,
    },
    DuplicateBinop {
        name: &'a str,
        first: Region,
        second: Region,
    },
    DuplicateUnionArg {
        type_name: &'a str,
        arg_name: &'a str,
        first: Region,
        second: Region,
    },
    DuplicateAliasArg {
        type_name: &'a str,
        arg_name: &'a str,
        first: Region,
        second: Region,
    },
    RecursiveAlias {
        region: Region,
        name: &'a str,
        args: &'a [&'a str],
        others: &'a [&'a str],
    },
    TypeVarsUnboundInUnion {
        region: Region,
        name: &'a str,
        args: &'a [&'a str],
        unbound: (&'a str, Region),
        more_unbound: &'a [(&'a str, Region)],
    },
    TypeVarsMessedUpInAlias {
        region: Region,
        name: &'a str,
        args: &'a [&'a str],
        unused: &'a [(&'a str, Region)],
        unbound: &'a [(&'a str, Region)],
    },
    DuplicateField {
        name: &'a str,
        first: Region,
        second: Region,
    },
    ExportDuplicate {
        name: &'a str,
        first: Region,
        second: Region,
    },
}
