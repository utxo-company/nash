use std::collections::BTreeMap;

use nash_region::Region;

use crate::Error;

pub fn detect<'a>(
    items: impl IntoIterator<Item = (&'a str, Region)>,
    to_error: impl Fn(&'a str, Region, Region) -> Error<'a>,
) -> Result<BTreeMap<&'a str, Region>, Vec<Error<'a>>> {
    let mut seen: BTreeMap<&'a str, Region> = BTreeMap::new();
    let mut errors: Vec<Error<'a>> = Vec::new();

    for (name, region) in items {
        if let Some(&prev_region) = seen.get(name) {
            errors.push(to_error(name, prev_region, region));
        } else {
            seen.insert(name, region);
        }
    }

    if errors.is_empty() {
        Ok(seen)
    } else {
        Err(errors)
    }
}
