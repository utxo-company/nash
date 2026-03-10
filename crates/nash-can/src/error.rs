#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    MissingModuleHeader,
    UnsupportedImports,
    UnsupportedValues,
    UnsupportedUnions,
    UnsupportedAliases,
}
