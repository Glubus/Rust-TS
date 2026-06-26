use std::collections::BTreeMap;

pub(super) struct ObjectTree<T> {
    pub(super) roots: BTreeMap<String, ObjectNode<T>>,
}

impl<T> Default for ObjectTree<T> {
    fn default() -> Self {
        Self {
            roots: BTreeMap::new(),
        }
    }
}

impl<T> ObjectTree<T> {
    pub(super) fn insert(&mut self, path: &str, binding: T) {
        let mut segments = path.split('.').collect::<Vec<_>>();
        let Some(root) = first_segment(&mut segments) else {
            return;
        };

        self.roots
            .entry(root.to_owned())
            .or_default()
            .insert(&segments, binding);
    }

    pub(super) fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }
}

pub(super) struct ObjectNode<T> {
    pub(super) children: BTreeMap<String, ObjectNode<T>>,
    pub(super) binding: Option<T>,
}

impl<T> Default for ObjectNode<T> {
    fn default() -> Self {
        Self {
            children: BTreeMap::new(),
            binding: None,
        }
    }
}

impl<T> ObjectNode<T> {
    fn insert(&mut self, segments: &[&str], binding: T) {
        let Some((head, tail)) = segments.split_first() else {
            self.binding = Some(binding);
            return;
        };

        self.children
            .entry((*head).to_owned())
            .or_default()
            .insert(tail, binding);
    }
}

fn first_segment<'a>(segments: &mut Vec<&'a str>) -> Option<&'a str> {
    if segments.is_empty() {
        return None;
    }

    Some(segments.remove(0))
}
