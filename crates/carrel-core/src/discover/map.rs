//! The archive map: collections by total size and number of items, as a tree, for a treemap
//! in the app and a sized tree in the terminal.

use serde::{Deserialize, Serialize};

use super::Catalog;

/// A collection on the map, with the collections inside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Region {
    /// Its folder, relative to the archive root.
    pub path: String,
    /// Its title.
    pub title: String,
    /// The items directly in it.
    pub items: usize,
    /// Their size in bytes, as their cards record it.
    pub size: u64,
    /// The items in it and every collection below it.
    pub total_items: usize,
    /// Their size in bytes.
    pub total_size: u64,
    /// The collections directly inside it, largest first.
    pub children: Vec<Region>,
}

/// The archive's map.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Map {
    /// The top-level collections, largest first.
    pub regions: Vec<Region>,
    /// Every item in the archive.
    pub total_items: usize,
    /// Their size in bytes.
    pub total_size: u64,
    /// How many items' cards record no size, so aren't counted in the sizes.
    pub without_size: usize,
}

/// The collection of `paths` that `child` is directly inside, if any: the nearest one above
/// it.
fn parent_of<'a>(child: &str, paths: &[&'a str]) -> Option<&'a str> {
    paths
        .iter()
        .filter(|p| {
            child.len() > p.len() && child.starts_with(*p) && child[p.len()..].starts_with('/')
        })
        .max_by_key(|p| p.len())
        .copied()
}

fn region(catalog: &Catalog, path: &str, paths: &[&str]) -> Region {
    let (items, size) = catalog
        .in_collection(path)
        .fold((0, 0), |(n, s), e| (n + 1, s + e.size.unwrap_or(0)));
    let mut children: Vec<Region> = paths
        .iter()
        .filter(|p| parent_of(p, paths) == Some(path))
        .map(|p| region(catalog, p, paths))
        .collect();
    sort(&mut children);
    Region {
        path: path.to_owned(),
        title: catalog
            .collection(path)
            .map_or_else(|| path.to_owned(), |c| c.title.clone()),
        items,
        size,
        total_items: items + children.iter().map(|c| c.total_items).sum::<usize>(),
        total_size: size + children.iter().map(|c| c.total_size).sum::<u64>(),
        children,
    }
}

fn sort(regions: &mut [Region]) {
    regions.sort_by(|a, b| {
        b.total_size
            .cmp(&a.total_size)
            .then(b.total_items.cmp(&a.total_items))
            .then(a.path.cmp(&b.path))
    });
}

/// The map of `catalog`'s collections.
#[must_use]
pub fn map(catalog: &Catalog) -> Map {
    let paths: Vec<&str> = catalog
        .collections
        .iter()
        .map(|c| c.path.as_str())
        .collect();
    let mut regions: Vec<Region> = paths
        .iter()
        .filter(|p| parent_of(p, &paths).is_none())
        .map(|p| region(catalog, p, &paths))
        .collect();
    sort(&mut regions);
    Map {
        total_items: regions.iter().map(|r| r.total_items).sum(),
        total_size: regions.iter().map(|r| r.total_size).sum(),
        without_size: catalog.entries.iter().filter(|e| e.size.is_none()).count(),
        regions,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discover::tests::{collection, entry};

    #[test]
    fn collections_are_sized_with_what_is_inside_them() {
        let mut a = entry(1, "a", "Projects");
        a.size = Some(100);
        let mut b = entry(2, "b", "Projects/Hardware");
        b.size = Some(500);
        let c = entry(3, "c", "Projects/Hardware/Old");
        let mut d = entry(4, "d", "Reading");
        d.size = Some(50);
        let catalog = Catalog::new(
            vec![a, b, c, d],
            [
                "Projects",
                "Projects/Hardware",
                "Projects/Hardware/Old",
                "Reading",
                "Holding",
            ]
            .map(collection)
            .to_vec(),
        );
        let map = map(&catalog);
        let top: Vec<(&str, usize, u64)> = map
            .regions
            .iter()
            .map(|r| (r.path.as_str(), r.total_items, r.total_size))
            .collect();
        assert_eq!(
            top,
            [("Projects", 3, 600), ("Reading", 1, 50), ("Holding", 0, 0)]
        );
        let projects = &map.regions[0];
        assert_eq!((projects.items, projects.size), (1, 100));
        assert_eq!(
            projects.children.len(),
            1,
            "Old is inside Hardware, not Projects"
        );
        let hardware = &projects.children[0];
        assert_eq!((hardware.total_items, hardware.total_size), (2, 500));
        assert_eq!(hardware.children[0].path, "Projects/Hardware/Old");
        assert_eq!(
            (map.total_items, map.total_size, map.without_size),
            (4, 650, 1)
        );
    }
}
