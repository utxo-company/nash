use nash_region::Region;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeKind {
    Alias,
    Union,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error<'a> {
    MissingModuleHeader,
    UnresolvedNamedType {
        region: Region,
        name: &'a str,
    },
    UnresolvedQualifiedNamedType {
        region: Region,
        module: &'a str,
        name: &'a str,
    },
    MissingImportedInterface {
        region: Region,
        module: &'a str,
    },
    AmbiguousImportedType {
        region: Region,
        name: &'a str,
        first_module: &'a str,
        second_module: &'a str,
    },
    BadTypeArity {
        region: Region,
        kind: TypeKind,
        name: &'a str,
        expected: usize,
        actual: usize,
    },
    UnresolvedExportedUpperName {
        region: Region,
        name: &'a str,
    },
}
