use drax::nbt::Tag;
use mcprotocol::common::chat::Chat;
use mcprotocol::{combine, lock_static, msg};
use serde_derive::{Deserialize, Serialize};
use shovel::inventory::item::ItemBuilder;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GripItem {
    ordinal: usize,
    item_name: Chat,
    item_lore_parts: Vec<Chat>,
    item_path: String,
    damage: u128,
}

impl GripItem {
    pub fn create_item(&self) -> ItemBuilder {
        ItemBuilder::new(self.item_path.as_str())
            .display_name(self.item_name.clone())
            .add_all_lore(vec![
                msg!(""),
                msg!("Description: ", "red").bold(true).italic(false),
            ])
            .add_all_lore(self.item_lore_parts.clone())
            .add_all_lore(vec![
                msg!(""),
                combine!(
                    msg!("Damage: ", "red").bold(true).italic(false),
                    msg!(format!("{}", self.damage), "white")
                        .bold(false)
                        .italic(false)
                ),
            ])
            .add_nbt("HideFlags", Tag::TagInt(127))
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct CacheRegistry {
    items: Vec<GripItem>,
}

pub struct GripItemRegistry {
    pub available_items: HashMap<usize, GripItem>,
}

lock_static!(GRIP_ITEM_REGISTRY -> GripItemRegistry => create);

impl GripItemRegistry {
    pub fn create() -> Self {
        let available_items =
            serde_json::from_slice::<CacheRegistry>(include_bytes!("./grip-item-reg.json"))
                .unwrap()
                .items
                .into_iter()
                .map(|item| (item.ordinal, item))
                .collect();

        Self { available_items }
    }

    pub fn get(&self, ordinal: usize) -> Option<&GripItem> {
        self.available_items.get(&ordinal)
    }

    pub fn get_all(&self) -> impl Iterator<Item = &GripItem> {
        self.available_items.values()
    }
}
