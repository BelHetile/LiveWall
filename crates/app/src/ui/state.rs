use crate::model::ItemKind;
use crate::ui::view::{Library, Target};
use crate::ui::wall::duration;

pub const TAG_ALL: isize = -1;
pub const TAG_NEW_FOLDER: isize = -2;
pub const TAG_RESET: isize = -100;
pub const TAG_LOCK_FOLDER: isize = 1;
pub const TAG_UNLOCK_FOLDER: isize = 2;
pub const TAG_HIDE_FOLDER: isize = 3;

pub fn folder_glyph(library: &Library, folder: usize) -> Option<&'static str> {
    let lock = library.locks.get(folder)?;
    return lock.locked.then_some(if library.unlocked { "lock.open.fill" } else { "lock.fill" });
}

pub fn privacy_title(unlocked: bool) -> &'static str {
    return if unlocked { "Lock Private Folders" } else { "Unlock Private Folders…" };
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    All,
    Space(usize),
}

impl Scope {
    pub fn title(self) -> String {
        return match self {
            Scope::All => "All Desktops".to_string(),
            Scope::Space(space) => format!("Desktop {}", space + 1),
        };
    }

    pub fn tag(self) -> isize {
        return match self {
            Scope::All => TAG_ALL,
            Scope::Space(space) => isize::try_from(space).unwrap_or(TAG_ALL),
        };
    }

    pub fn from_tag(value: isize) -> Self {
        return usize::try_from(value).map_or(Scope::All, Scope::Space);
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    NameAscending,
    NameDescending,
    Newest,
    Oldest,
}

impl Sort {
    pub const ALL: [(Sort, &str); 4] = [
        (Sort::NameAscending, "Name (A–Z)"),
        (Sort::NameDescending, "Name (Z–A)"),
        (Sort::Newest, "Date Added (Newest)"),
        (Sort::Oldest, "Date Added (Oldest)"),
    ];
}

pub const KINDS: [(Option<ItemKind>, &str); 4] = [
    (None, "All Kinds"),
    (Some(ItemKind::Video), "Videos"),
    (Some(ItemKind::Gif), "GIFs"),
    (Some(ItemKind::Image), "Images"),
];

pub struct Filter {
    pub scope: Scope,
    pub folder: Option<usize>,
    pub sort: Sort,
    pub kind: Option<ItemKind>,
    pub query: String,
    pub list: bool,
}

impl Filter {
    pub fn visible(&self, library: &Library) -> Vec<usize> {
        let query = self.query.trim().to_lowercase();
        let mut indices: Vec<usize> = library
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| self.folder.is_none_or(|folder| item.folder == Some(folder)))
            .filter(|(_, item)| self.kind.is_none_or(|kind| item.kind == kind))
            .filter(|(_, item)| query.is_empty() || item.title.to_lowercase().contains(&query))
            .map(|(index, _)| index)
            .collect();
        let items = &library.items;
        match self.sort {
            Sort::NameAscending => indices.sort_by_key(|index| items[*index].title.to_lowercase()),
            Sort::NameDescending => {
                indices.sort_by_key(|index| std::cmp::Reverse(items[*index].title.to_lowercase()));
            }
            Sort::Newest => indices.sort_by_key(|index| std::cmp::Reverse(items[*index].added)),
            Sort::Oldest => indices.sort_by_key(|index| items[*index].added),
        }
        return indices;
    }
}

pub struct Showing {
    pub main: Option<usize>,
    pub others: usize,
    pub desktops: Vec<(usize, Option<usize>)>,
}

pub fn showing(library: &Library, scope: Scope) -> Showing {
    let desktops = library.desktops(scope);
    let mut counts: Vec<(usize, usize)> = Vec::new();
    for item in desktops.iter().filter_map(|(_, item)| *item) {
        match counts.iter_mut().find(|(candidate, _)| *candidate == item) {
            Some((_, count)) => *count += 1,
            None => counts.push((item, 1)),
        }
    }
    let main = counts.iter().fold(None::<(usize, usize)>, |best, &(item, count)| match best {
        Some((_, top)) if top >= count => best,
        _ => Some((item, count)),
    });
    return Showing { main: main.map(|(item, _)| item), others: counts.len().saturating_sub(1), desktops };
}

pub struct Marker {
    pub icon: &'static str,
    pub text: String,
    pub tip: String,
    pub active: bool,
}

pub fn markers(library: &Library, item: usize, scope: Scope) -> Vec<Marker> {
    let targets = library.markers(item);
    let showing = library.showing(scope);
    let mine: Vec<Target> = targets.iter().copied().filter(|target| *target != Target::Lock).collect();
    let mut out = Vec::new();
    if !mine.is_empty() {
        let spaces: Vec<String> = mine
            .iter()
            .filter_map(|target| match target {
                Target::Desktop(space) => Some((space + 1).to_string()),
                _ => None,
            })
            .collect();
        let all = mine.contains(&Target::All);
        let short = if all { "All".to_string() } else { spaces.join(", ") };
        let long = match (all, spaces.len()) {
            (true, _) => "All Desktops".to_string(),
            (false, 1) => format!("Desktop {short}"),
            (false, _) => format!("Desktops {short}"),
        };
        let active = mine.iter().any(|target| showing.contains(target));
        out.push(Marker { icon: "display", text: short, tip: long, active });
    }
    if targets.contains(&Target::Lock) {
        out.push(Marker { icon: "lock.fill", text: "Lock".to_string(), tip: lock_tip(library), active: false });
    }
    return out;
}

pub fn lock_tip(library: &Library) -> String {
    return library
        .lock
        .as_ref()
        .filter(|lock| lock.time > 0.0)
        .map_or_else(|| "Lock Screen".to_string(), |lock| format!("Lock Screen · Frame at {}", duration(lock.time)));
}
