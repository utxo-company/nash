use nash_ast::{ModuleName, Type as CanType};
use nash_region::Located;

#[derive(Clone, Copy, Debug)]
pub struct Interface<'a> {
    pub home: ModuleName<'a>,
    pub aliases: &'a [InterfaceAlias<'a>],
    pub unions: &'a [InterfaceUnion<'a>],
}

#[derive(Clone, Copy, Debug)]
pub struct InterfaceAlias<'a> {
    pub name: &'a str,
    pub parameters: &'a [&'a str],
    pub typ: &'a Located<CanType<'a>>,
}

#[derive(Clone, Copy, Debug)]
pub struct InterfaceUnion<'a> {
    pub name: &'a str,
    pub parameters: &'a [&'a str],
}
