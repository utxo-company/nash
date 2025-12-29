use nash_region::{Located, Region};

pub struct Module<'a> {
    pub name: Option<&'a Located<&'a str>>,
    pub exports: &'a Located<Exposing<'a>>,
    pub docs: &'a Docs<'a>,
    pub imports: &'a [&'a Import<'a>],
    pub values: &'a [&'a Located<Value<'a>>],
    pub unions: &'a [&'a Located<Union<'a>>],
    pub aliases: &'a [&'a Located<Alias<'a>>],
    pub binops: &'a [&'a Located<Infix<'a>>],
}

pub struct Import<'a> {
    pub import: &'a Located<&'a str>,
    pub alias: Option<&'a str>,
    pub exposing: &'a Exposing<'a>,
}

pub struct Value<'a> {
    pub name: &'a Located<&'a str>,
    pub arguments: &'a [&'a Located<Pattern<'a>>],
    pub body: &'a Located<Expr<'a>>,
    pub annotation: Option<&'a Located<Type<'a>>>,
}

// type Maybe a
//   = Just a
//   | Nothing
pub struct Union<'a> {
    pub name: &'a Located<&'a str>,
    // type vars
    pub arguments: &'a [&'a Located<&'a str>],
    pub ctors: &'a [&'a Ctor<'a>],
}

pub struct Ctor<'a> {
    pub name: &'a Located<&'a str>,
    pub arguments: &'a [&'a Located<Type<'a>>],
}

pub struct Alias<'a> {
    pub name: &'a Located<&'a str>,
    // type vars
    pub arguments: &'a [&'a Located<&'a str>],
    pub typ: &'a Located<Type<'a>>,
}

pub struct Infix<'a> {
    pub op: &'a str,
    pub associativity: Associativity,
    pub precedence: Precedence,
    pub name: &'a str,
}

#[derive(PartialEq, Eq)]
pub enum Associativity {
    Left,
    None,
    Right,
}

#[derive(PartialEq, Eq, PartialOrd, Ord)]
pub struct Precedence(pub u16);

#[derive(Debug)]
pub enum Expr<'a> {
    Str(&'a str),
    Int(i128),
    Var {
        kind: VarType,
        name: &'a str,
    },
    VarQual {
        kind: VarType,
        module: &'a str,
        name: &'a str,
    },
    List(&'a [&'a Located<Expr<'a>>]),
    Op(&'a str),
    Negate(&'a Located<Expr<'a>>),
    BinOps(
        &'a [&'a (&'a Located<Expr<'a>>, &'a Located<&'a str>)],
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
    Let(&'a [&'a Located<Def<'a>>], &'a Located<Expr<'a>>),
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

#[derive(Debug)]
pub enum VarType {
    LowVar,
    CapVar,
}

#[derive(Debug)]
pub struct IfBranch<'a> {
    pub condition: &'a Located<Expr<'a>>,
    pub body: &'a Located<Expr<'a>>,
}

#[derive(Debug)]
pub enum Def<'a> {
    Define(
        &'a Located<&'a str>,
        &'a [&'a Located<Pattern<'a>>],
        &'a Located<Expr<'a>>,
        Option<&'a Located<Type<'a>>>,
    ),
    Destruct(&'a Located<Pattern<'a>>, &'a Located<Expr<'a>>),
}

#[derive(Debug)]
pub struct CaseArm<'a> {
    pub pattern: &'a Located<Pattern<'a>>,
    pub body: &'a Located<Expr<'a>>,
}

#[derive(Debug)]
pub struct FieldAssign<'a> {
    pub field: &'a Located<&'a str>,
    pub value: &'a Located<Expr<'a>>,
}

#[derive(Debug)]
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
    Ctor(Region, &'a str, &'a [&'a Located<Pattern<'a>>]),
    CtorQual(Region, &'a str, &'a str, &'a [&'a Located<Pattern<'a>>]),
    List(&'a [&'a Located<Pattern<'a>>]),
    Cons(&'a Located<Pattern<'a>>, &'a Located<Pattern<'a>>),
    Str(&'a str),
    Int(i128),
}

#[derive(Debug)]
pub enum Type<'a> {
    Lambda(&'a Located<Type<'a>>, &'a Located<Type<'a>>),
    Var(&'a str),
    Type(Region, &'a str, &'a [&'a Located<Type<'a>>]),
    TypeQual(Region, &'a str, &'a str, &'a [&'a Located<Type<'a>>]),
    Record(&'a [&'a FieldType<'a>], Option<&'a Located<&'a str>>),
    Unit,
    Tuple(
        &'a Located<Type<'a>>,
        &'a Located<Type<'a>>,
        &'a [&'a Located<Type<'a>>],
    ),
}

#[derive(Debug)]
pub struct FieldType<'a> {
    pub field: &'a Located<&'a str>,
    pub typ: &'a Located<Type<'a>>,
}

pub enum Docs<'a> {
    NoDocs(Region),
    YesDocs(&'a Comment<'a>, &'a [&'a (&'a str, &'a Comment<'a>)]),
}

pub struct Comment<'a>(pub &'a Snippet<'a>);

pub struct Snippet<'a> {
    pub data: &'a [u8], // already the relevant slice
    // offset: usize,
    // length: usize,
    pub off_row: u16,
    pub off_col: u16,
}

pub enum Exposing<'a> {
    Open,
    Explicit(&'a [&'a Exposed<'a>]),
}

pub enum Exposed<'a> {
    Lower(&'a Located<&'a str>),
    Upper(&'a Located<&'a str>, Privacy),
    Operator(Region, &'a str),
}

pub enum Privacy {
    Public(Region),
    Private,
}
