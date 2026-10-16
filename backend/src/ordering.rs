//! Ordering of tasks, lists and wiki pages: integer positions spaced [`GAP`] apart. A new item goes last;
//! a move takes the midpoint of its new neighbours; when there is no whole number left between them, the
//! siblings are renumbered.

/// The space between neighbours after numbering.
pub const GAP: i64 = 20;

/// The position for a new item after `siblings` (any order).
pub fn append(siblings: impl IntoIterator<Item = i64>) -> i64 {
    siblings.into_iter().max().map_or(GAP, |max| max + GAP)
}

/// Where a moved item goes, and which siblings get new positions because there was no room.
#[derive(Debug, PartialEq, Eq)]
pub struct Placement {
    pub position: i64,
    /// Siblings that must be renumbered: (id, new position). Empty in the usual case.
    pub renumber: Vec<(uuid::Uuid, i64)>,
}

/// Place an item right after `after` among `siblings` (in order, not including the item), or first when
/// `after` is `None`. An unknown `after` puts it last.
pub fn place(siblings: &[(uuid::Uuid, i64)], after: Option<uuid::Uuid>) -> Placement {
    let index = match after {
        None => 0,
        Some(id) => siblings
            .iter()
            .position(|(s, _)| *s == id)
            .map_or(siblings.len(), |i| i + 1),
    };
    let before = index.checked_sub(1).map(|i| siblings[i].1);
    let next = siblings.get(index).map(|s| s.1);
    let gap = match (before, next) {
        (None, None) => Some(GAP),
        (Some(b), None) => Some(b + GAP),
        (None, Some(n)) => (n > 1).then_some(n / 2).filter(|p| *p >= 1 && *p < n),
        (Some(b), Some(n)) => (n - b >= 2).then_some(b + (n - b) / 2),
    };
    if let Some(position) = gap {
        return Placement {
            position,
            renumber: vec![],
        };
    }
    // No room: renumber everything, leaving the moved item's slot at `index`.
    let mut renumber = vec![];
    let mut position = GAP;
    let mut slot = 0;
    for (i, (id, _)) in siblings.iter().enumerate() {
        if i == index {
            slot = position;
            position += GAP;
        }
        renumber.push((*id, position));
        position += GAP;
    }
    if index == siblings.len() {
        slot = position;
    }
    Placement {
        position: slot,
        renumber,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(n: u128) -> Vec<uuid::Uuid> {
        (1..=n).map(uuid::Uuid::from_u128).collect()
    }

    #[test]
    fn appends_after_the_last() {
        assert_eq!(append([]), 20);
        assert_eq!(append([20, 60, 40]), 80);
    }

    #[test]
    fn places_between_neighbours_without_touching_them() {
        let i = ids(3);
        let siblings = [(i[0], 20), (i[1], 40), (i[2], 60)];
        assert_eq!(place(&siblings, Some(i[0])).position, 30);
        assert_eq!(place(&siblings, None).position, 10);
        assert_eq!(place(&siblings, Some(i[2])).position, 80);
        assert!(place(&siblings, Some(i[1])).renumber.is_empty());
    }

    #[test]
    fn renumbers_when_there_is_no_room() {
        let i = ids(3);
        let siblings = [(i[0], 20), (i[1], 21), (i[2], 60)];
        let p = place(&siblings, Some(i[0]));
        assert_eq!(p.position, 40);
        assert_eq!(p.renumber, vec![(i[0], 20), (i[1], 60), (i[2], 80)]);
        // At the very start with position 1 taken.
        let p = place(&[(i[0], 1)], None);
        assert_eq!(p.position, 20);
        assert_eq!(p.renumber, vec![(i[0], 40)]);
    }
}
