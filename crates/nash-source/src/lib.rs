use nash_region::{Located, Region};

pub enum Expr<'a> {
    Str(&'a str),
    Int(i128),
    Var(VarType, &'a str),
    VarQual(VarType, &'a str, &'a str),
    List(&'a [&'a Located<Expr<'a>>]),
    Op(&'a str),
    Negate(&'a Located<Expr<'a>>),
    BinOps(
        &'a [(&'a Located<Expr<'a>>, &'a Located<&'a str>)],
        &'a Located<Expr<'a>>,
    ),
    Lambda {
        parameters: &'a [&'a Located<Pattern<'a>>],
        body: &'a Located<Expr<'a>>,
    },
    Call {
        function: &'a Located<Expr<'a>>,
        arguments: &'a [&'a Located<Expr<'a>>],
    },
    If {
        branches: &'a [&'a IfBranch<'a>],
        final_else: &'a Located<Expr<'a>>,
    },
    Let(&'a [&'a Located<Expr<'a>>], &'a Located<Expr<'a>>),
    Case {
        scrutinee: &'a Located<Expr<'a>>,
        arms: &'a [&'a CaseArm<'a>],
    },
    Accessor(&'a str),
    Update(&'a Located<&'a str>, &'a [&'a FieldAssign<'a>]),
    Record(&'a [&'a FieldAssign<'a>]),
    Unit,
    Tuple(
        &'a Located<Expr<'a>>,
        &'a Located<Expr<'a>>,
        &'a [&'a Located<Expr<'a>>],
    ),
}

pub enum VarType {
    LowVar,
    CapVar,
}

pub struct IfBranch<'a> {
    pub condition: &'a Located<Expr<'a>>,
    pub body: &'a Located<Expr<'a>>,
}

pub struct CaseArm<'a> {
    pub pattern: &'a Located<Pattern<'a>>,
    pub body: &'a Located<Expr<'a>>,
}

pub struct FieldAssign<'a> {
    pub field: &'a Located<&'a str>,
    pub value: &'a Located<Expr<'a>>,
}

pub enum Pattern<'a> {
    Anything,
    Var(&'a str),
    Record(&'a [&'a Located<&'a str>]),
    Alias(&'a Located<Pattern<'a>>, &'a Located<&'a str>),
    Unit,
    Tuple(
        &'a Located<Pattern<'a>>,
        &'a Located<Pattern<'a>>,
        &'a [&'a Located<Pattern<'a>>],
    ),
    Ctor(&'a Region, &'a str, &'a [&'a Located<Pattern<'a>>]),
    CtorQual(&'a Region, &'a str, &'a str, &'a [&'a Located<Pattern<'a>>]),
    List(&'a [&'a Located<Pattern<'a>>]),
    Cons(&'a Located<Pattern<'a>>, &'a Located<Pattern<'a>>),
    Str(&'a str),
    Int(i128),
}
