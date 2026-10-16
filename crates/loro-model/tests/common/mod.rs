//! The plan's example schema plus the code `#[derive(LoroModel)]` is expected
//! to emit for it. Everything below each `// ---- generated` marker uses only
//! what the macro sees: field names, field types, and `#[loro(...)]` attributes.
//! It never inspects another type's definition; cross-type information flows
//! through `LoroSchema::{View, VecView}` and `ToLoro::KEY_FIELD`.
#![allow(dead_code)]

use loro::{ContainerType, LoroMap, LoroValue, ReadEntry, StateReader};
use loro_model::{
    container_of, field_same_as, read_field, read_field_in, struct_map, FromLoro, Loc, LoroSchema,
    ModelError, Result, Slot, TextField, ToLoro, View,
};

// #[derive(Clone, Debug, PartialEq, LoroModel)]
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Project {
    pub title: String,
    pub todos: Vec<Todo>,
    // #[loro(text)]
    pub notes: String,
}

// #[derive(Clone, Debug, PartialEq, LoroModel)]
#[derive(Clone, Debug, PartialEq)]
pub struct Todo {
    // #[loro(key)]
    pub id: String,
    pub title: String,
    pub done: bool,
}

// #[derive(Debug, PartialEq, FromLoro)]
#[derive(Debug, PartialEq)]
pub struct TodoSummary {
    pub id: String,
    pub title: String,
}

// ---- generated for Project

impl FromLoro for Project {
    fn from_loro(v: &LoroValue) -> Result<Self> {
        let LoroValue::Map(m) = v else {
            return Err(ModelError::mismatch("map", v));
        };
        Ok(Self {
            title: read_field(m, "title", String::from_loro, String::from_missing)?,
            todos: read_field(
                m,
                "todos",
                Vec::<Todo>::from_loro,
                Vec::<Todo>::from_missing,
            )?,
            notes: read_field(
                m,
                "notes",
                loro_model::text::from_loro,
                String::from_missing,
            )?,
        })
    }

    fn read<'r>(r: &mut StateReader<'r>, e: ReadEntry<'r>) -> Result<Self> {
        if let ReadEntry::Value(v) = &e {
            return Self::from_loro(v);
        }
        let Some(m) = struct_map(&e)? else {
            unreachable!("value entries are handled above");
        };
        Ok(Self {
            title: read_field_in(r, m, "title", String::read, String::from_missing)?,
            todos: read_field_in(r, m, "todos", Vec::<Todo>::read, Vec::<Todo>::from_missing)?,
            notes: read_field_in(r, m, "notes", loro_model::text::read, String::from_missing)?,
        })
    }
}

impl ToLoro for Project {
    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        // Struct maps keep keys they do not know (forward compatibility).
        let map = slot.ensure_map()?;
        ToLoro::to_loro(&self.title, Slot::MapKey(&map, "title")).map_err(|e| e.in_key("title"))?;
        ToLoro::to_loro(&self.todos, Slot::MapKey(&map, "todos")).map_err(|e| e.in_key("todos"))?;
        loro_model::text::to_loro(&self.notes, Slot::MapKey(&map, "notes"))
            .map_err(|e| e.in_key("notes"))?;
        Ok(())
    }

    fn same_as<'r>(&self, r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool {
        let Some(m) = container_of(cur, ContainerType::Map) else {
            return false;
        };
        field_same_as(r, m, "title", |r, c| self.title.same_as(r, c))
            && field_same_as(r, m, "todos", |r, c| self.todos.same_as(r, c))
            && field_same_as(r, m, "notes", |r, c| {
                loro_model::text::same_as(&self.notes, r, c)
            })
    }
}

impl LoroSchema for Project {
    type View = ProjectModel;
    type VecView = loro_model::ListView<Self>;
}

pub struct ProjectModel {
    loc: Loc,
}

impl View for ProjectModel {
    fn from_loc(loc: Loc) -> Self {
        Self { loc }
    }
    fn loc(&self) -> &Loc {
        &self.loc
    }
}

impl ProjectModel {
    pub fn attach(map: LoroMap) -> Self {
        Self::from_loc(Loc::root(loro::ContainerTrait::to_container(&map)))
    }

    pub fn title(&self) -> <String as LoroSchema>::View {
        View::from_loc(Loc::field(&self.loc, "title"))
    }

    pub fn todos(&self) -> <Vec<Todo> as LoroSchema>::View {
        View::from_loc(Loc::field(&self.loc, "todos"))
    }

    pub fn notes(&self) -> TextField {
        View::from_loc(Loc::field(&self.loc, "notes"))
    }
}

// ---- generated for Todo

impl FromLoro for Todo {
    fn from_loro(v: &LoroValue) -> Result<Self> {
        let LoroValue::Map(m) = v else {
            return Err(ModelError::mismatch("map", v));
        };
        Ok(Self {
            id: read_field(m, "id", String::from_loro, String::from_missing)?,
            title: read_field(m, "title", String::from_loro, String::from_missing)?,
            done: read_field(m, "done", bool::from_loro, bool::from_missing)?,
        })
    }

    fn read<'r>(r: &mut StateReader<'r>, e: ReadEntry<'r>) -> Result<Self> {
        if let ReadEntry::Value(v) = &e {
            return Self::from_loro(v);
        }
        let Some(m) = struct_map(&e)? else {
            unreachable!("value entries are handled above");
        };
        Ok(Self {
            id: read_field_in(r, m, "id", String::read, String::from_missing)?,
            title: read_field_in(r, m, "title", String::read, String::from_missing)?,
            done: read_field_in(r, m, "done", bool::read, bool::from_missing)?,
        })
    }
}

impl ToLoro for Todo {
    const KEY_FIELD: Option<&'static str> = Some("id");

    fn key(&self) -> Option<&str> {
        Some(&self.id)
    }

    fn to_loro(&self, slot: Slot<'_>) -> Result<()> {
        let map = slot.ensure_map()?;
        ToLoro::to_loro(&self.id, Slot::MapKey(&map, "id")).map_err(|e| e.in_key("id"))?;
        ToLoro::to_loro(&self.title, Slot::MapKey(&map, "title")).map_err(|e| e.in_key("title"))?;
        ToLoro::to_loro(&self.done, Slot::MapKey(&map, "done")).map_err(|e| e.in_key("done"))?;
        Ok(())
    }

    fn same_as<'r>(&self, r: &mut StateReader<'r>, cur: Option<&ReadEntry<'r>>) -> bool {
        let Some(m) = container_of(cur, ContainerType::Map) else {
            return false;
        };
        field_same_as(r, m, "id", |r, c| self.id.same_as(r, c))
            && field_same_as(r, m, "title", |r, c| self.title.same_as(r, c))
            && field_same_as(r, m, "done", |r, c| self.done.same_as(r, c))
    }
}

impl LoroSchema for Todo {
    type View = TodoModel;
    // Has a #[loro(key)] field.
    type VecView = loro_model::KeyedVec<Self>;
}

pub struct TodoModel {
    loc: Loc,
}

impl View for TodoModel {
    fn from_loc(loc: Loc) -> Self {
        Self { loc }
    }
    fn loc(&self) -> &Loc {
        &self.loc
    }
}

impl TodoModel {
    /// Key fields are read-only through views: the key is the element's
    /// identity (its map key in the keyed vec).
    pub fn id(&self) -> Result<String> {
        View::read_as(&<String as LoroSchema>::View::from_loc(Loc::field(
            &self.loc, "id",
        )))
    }

    pub fn title(&self) -> <String as LoroSchema>::View {
        View::from_loc(Loc::field(&self.loc, "title"))
    }

    pub fn done(&self) -> <bool as LoroSchema>::View {
        View::from_loc(Loc::field(&self.loc, "done"))
    }
}

// ---- generated for TodoSummary (FromLoro only)

impl FromLoro for TodoSummary {
    fn from_loro(v: &LoroValue) -> Result<Self> {
        let LoroValue::Map(m) = v else {
            return Err(ModelError::mismatch("map", v));
        };
        Ok(Self {
            id: read_field(m, "id", String::from_loro, String::from_missing)?,
            title: read_field(m, "title", String::from_loro, String::from_missing)?,
        })
    }

    fn read<'r>(r: &mut StateReader<'r>, e: ReadEntry<'r>) -> Result<Self> {
        if let ReadEntry::Value(v) = &e {
            return Self::from_loro(v);
        }
        let Some(m) = struct_map(&e)? else {
            unreachable!("value entries are handled above");
        };
        Ok(Self {
            id: read_field_in(r, m, "id", String::read, String::from_missing)?,
            title: read_field_in(r, m, "title", String::read, String::from_missing)?,
        })
    }
}

// ---- test helpers

pub fn todo(id: &str, title: &str) -> Todo {
    Todo {
        id: id.into(),
        title: title.into(),
        done: false,
    }
}
