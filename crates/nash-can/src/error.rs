use nash_ast::ModuleName;
use nash_region::{Located, Region};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BadArityContext {
    TypeArity,
    PatternArity,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplicatePatternContext<'a> {
    LambdaArgs,
    FuncArgs(&'a str),
    CaseBranch,
    LetBinding,
    Destruct,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarKind {
    BadOp,
    BadVar,
    BadPattern,
    BadType,
}

/// Mirrors Elm's `Error.PossibleNames`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PossibleNames<'a> {
    pub locals: &'a [&'a str],
    pub qualified: &'a [(&'a str, &'a [&'a str])],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error<'a> {
    MissingModuleHeader,
    NotFoundType {
        region: Region,
        prefix: Option<&'a str>,
        name: &'a str,
        suggestions: PossibleNames<'a>,
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
    NotFoundCtor {
        region: Region,
        prefix: Option<&'a str>,
        name: &'a str,
        suggestions: PossibleNames<'a>,
    },
    AmbiguousCtor {
        region: Region,
        prefix: Option<&'a str>,
        name: &'a str,
        first_module: ModuleName<'a>,
        other_modules: &'a [ModuleName<'a>],
    },
    PatternHasRecordCtor {
        region: Region,
        name: &'a str,
    },
    DuplicatePattern {
        context: DuplicatePatternContext<'a>,
        name: &'a str,
        first: Region,
        second: Region,
    },
    TupleLargerThanThree {
        region: Region,
    },

    // --- Expression canonicalization errors ---
    NotFoundVar {
        region: Region,
        prefix: Option<&'a str>,
        name: &'a str,
        suggestions: PossibleNames<'a>,
    },
    AmbiguousVar {
        region: Region,
        prefix: Option<&'a str>,
        name: &'a str,
        first_module: ModuleName<'a>,
        other_modules: &'a [ModuleName<'a>],
    },
    NotFoundBinop {
        region: Region,
        name: &'a str,
        available: &'a [&'a str],
    },
    AmbiguousBinop {
        region: Region,
        name: &'a str,
        first_module: ModuleName<'a>,
        other_modules: &'a [ModuleName<'a>],
    },
    BinopConflict {
        region: Region,
        op1: &'a str,
        op2: &'a str,
    },
    Shadowing {
        name: &'a str,
        original: Region,
        new: Region,
    },
    RecursiveLet {
        name: &'a Located<&'a str>,
        others: &'a [&'a str],
    },
    AnnotationTooShort {
        region: Region,
        name: &'a str,
    },

    // --- Import validation errors ---
    ImportExposingNotFound {
        region: Region,
        module: ModuleName<'a>,
        name: &'a str,
        available: &'a [&'a str],
    },
    ImportCtorByName {
        region: Region,
        name: &'a str,
        type_name: &'a str,
    },
    ImportOpenAlias {
        region: Region,
        name: &'a str,
    },
}
