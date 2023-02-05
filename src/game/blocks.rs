use drax::prelude::Uuid;
use mcprotocol::clientbound::play::ClientboundPlayRegistry;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use mcprotocol::common::play::{BlockPos, ItemStack};
use mcprotocol::common::registry::RegistryKey;
use mcprotocol::{combine, lock_static, msg};
use rand::prelude::SliceRandom;
use shovel::entity::tracking::TrackableEntity;
use shovel::inventory::item::ItemBuilder;
use shovel::level::LevelMediator;
use shovel::phase::play::ConnectedPlayer;

use crate::game::session::GameSessionPlayer;

#[derive(serde_derive::Serialize, serde_derive::Deserialize, Debug)]
struct CacheRegistryItem {
    block_ordinal: usize,
    minecraft_block_tag: String,
    friendly_name: String,
    is_default: bool,
    initial_health: u128,
}

#[derive(serde_derive::Serialize, serde_derive::Deserialize, Debug)]
struct CacheRegistry {
    items: Vec<CacheRegistryItem>,
}

pub struct GlobalBlockRegistry {
    pub available_blocks: HashMap<AvailableBlockData, AvailableBlock>,
}

impl GlobalBlockRegistry {
    pub fn create() -> Self {
        let cached: CacheRegistry =
            serde_json::from_slice(include_bytes!("./blocks-reg.json")).unwrap();
        let mut available_blocks = HashMap::with_capacity(cached.items.len());
        for item in cached.items {
            let minecraft_block_tag = item.minecraft_block_tag;

            let data = AvailableBlockData {
                block_ordinal: item.block_ordinal,
                block_id: RegistryKey::BlockStates
                    .global(minecraft_block_tag.as_str())
                    .unwrap(),
                item_id: RegistryKey::Items
                    .global(minecraft_block_tag.as_str())
                    .unwrap(),
                initial_health: item.initial_health,
            };

            available_blocks.insert(
                data,
                AvailableBlock {
                    block_data: data,
                    friendly_name: item.friendly_name,
                    is_default: item.is_default,
                },
            );
        }

        Self { available_blocks }
    }

    pub fn search_by_ordinal(&self, ordinal: usize) -> Option<&AvailableBlock> {
        self.available_blocks
            .values()
            .find(|block| block.block_data.block_ordinal == ordinal)
    }

    pub fn get(&self, data: &AvailableBlockData) -> Option<&AvailableBlock> {
        self.available_blocks.get(data)
    }

    pub fn get_all(&self) -> impl Iterator<Item = &AvailableBlock> {
        self.available_blocks.values()
    }
}

lock_static!(GLOBAL_BLOCK_REGISTRY -> GlobalBlockRegistry => create);

#[derive(Clone, Copy, Debug, serde_derive::Serialize, serde_derive::Deserialize)]
pub struct AvailableBlockData {
    pub block_ordinal: usize,
    pub block_id: i32,
    pub item_id: i32,
    pub initial_health: u128,
}

impl Hash for AvailableBlockData {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.block_ordinal.hash(state)
    }
}

impl PartialEq for AvailableBlockData {
    fn eq(&self, other: &Self) -> bool {
        self.block_ordinal == other.block_ordinal
    }
}

impl Eq for AvailableBlockData {}

#[derive(Clone, Debug, serde_derive::Serialize, serde_derive::Deserialize)]
pub struct AvailableBlock {
    pub block_data: AvailableBlockData,
    pub friendly_name: String,
    pub is_default: bool,
}

impl AvailableBlock {
    pub fn create_item(&self, mined_count: u128) -> ItemStack {
        ItemBuilder::magic(self.block_data.item_id)
            .display_name(msg!(format!("{}", self.friendly_name), "aqua").bold(true))
            .add_all_lore(vec![
                msg!(""),
                combine!(
                    msg!("Mined: x", "aqua").bold(true),
                    msg!(format!("{}", mined_count), "green")
                ),
            ])
            .build()
    }
}

impl Ord for AvailableBlockData {
    fn cmp(&self, other: &Self) -> Ordering {
        self.block_ordinal.cmp(&other.block_ordinal)
    }
}

impl PartialOrd for AvailableBlockData {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Debug, serde_derive::Serialize, serde_derive::Deserialize, Default, PartialEq)]
pub struct PlayerBlockData {
    pub unlocked_blocks: Vec<AvailableBlockData>,
    pub mined_blocks: Vec<u128>,
    pub changed: bool,
}

fn default_rng() -> rand::rngs::StdRng {
    rand::SeedableRng::from_seed([0; 32])
}

pub struct BlockSystem {
    pub placed_blocks: HashMap<Uuid, HashMap<BlockPos, DamageableBlock>>,
    pub rand_state: rand::rngs::StdRng,
}

impl Default for BlockSystem {
    fn default() -> Self {
        Self {
            placed_blocks: HashMap::with_capacity(1),
            rand_state: default_rng(),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct DamageableBlock {
    pub block_data: AvailableBlockData,
    pub health: u128,
}

pub const BLOCK_BROKEN_FLAG: u8 = 255;

impl BlockSystem {
    pub fn current_state(
        &self,
        session: &mut ConnectedPlayer,
        pos: BlockPos,
    ) -> Option<AvailableBlockData> {
        Some(
            self.placed_blocks
                .get(&session.uuid())?
                .get(&pos)?
                .block_data,
        )
    }

    pub fn check_destroy(&mut self, session: &mut ConnectedPlayer, pos: BlockPos) -> bool {
        if let Some(placed) = self.placed_blocks.get(&session.uuid()) {
            if let Some(block) = placed.get(&pos) {
                if block.health == 0 {
                    log::info!(target: session.username().as_str(), "Destroy was a complete success!");
                    return true;
                }
            }
        }
        false
    }

    pub fn remove(
        &mut self,
        session: &mut ConnectedPlayer,
        pos: BlockPos,
    ) -> Option<AvailableBlockData> {
        self.placed_blocks
            .get_mut(&session.uuid())?
            .remove(&pos)
            .map(|b| b.block_data)
    }

    pub fn reset_progress(&mut self, session: &mut ConnectedPlayer, pos: BlockPos) -> Option<()> {
        let attacking = self.placed_blocks.get_mut(&session.uuid())?.get_mut(&pos)?;
        attacking.health = attacking.block_data.initial_health;
        Some(())
    }

    pub fn attempt_damage_block(
        &mut self,
        pos: BlockPos,
        session: &mut ConnectedPlayer,
    ) -> Option<(u128, u128)> {
        let attacking = self.placed_blocks.get_mut(&session.uuid())?.get_mut(&pos)?;
        if attacking.health == 0 {
            return Some((0, 0));
        }
        attacking.health -= 1;
        if attacking.health == 0 {
            return Some((0, 0));
        }

        Some((attacking.health, attacking.block_data.initial_health))
    }

    pub fn tick_for(&mut self, offset: BlockPos, session: &mut GameSessionPlayer) {
        let placed_blocks = self.placed_blocks.get_mut(&session.uuid());

        let placed_blocks = match placed_blocks {
            Some(placed_blocks) => placed_blocks,
            None => {
                self.placed_blocks.insert(session.uuid(), HashMap::new());
                self.placed_blocks.get_mut(&session.uuid()).unwrap()
            }
        };

        let mut mediator = LevelMediator::default();
        if session.current_tick % 20 == 0
            || session
                .state
                .player_destroying_state
                .destroying_block_sequence
                .is_some()
        {
            let placements = [
                BlockPos {
                    x: offset.x,
                    y: offset.y + 2,
                    z: offset.z,
                },
                BlockPos {
                    x: offset.x - 1,
                    y: offset.y + 2,
                    z: offset.z,
                },
            ];

            for placement in placements {
                if !placed_blocks.contains_key(&placement) {
                    log::info!("Populating placement: {:?}", placement);
                    let block_to_place = session
                        .block_data
                        .unlocked_blocks
                        .choose(&mut self.rand_state);
                    if let Some(block) = block_to_place {
                        mediator.update(placement, block.block_id);
                        placed_blocks.insert(
                            placement,
                            DamageableBlock {
                                block_data: *block,
                                health: block.initial_health,
                            },
                        );
                    }
                }
            }
        }

        if mediator.validate_positions(session) {
            for change in mediator.into_updates() {
                if let ClientboundPlayRegistry::SectionBlocksUpdate { update_info, .. } =
                    change.as_ref()
                {
                    if update_info.len() == 0 {
                        continue;
                    }
                    if update_info.len() == 1 {
                        let update = update_info[0];
                        session.write_owned_packet(ClientboundPlayRegistry::BlockUpdate {
                            pos: update.block_pos,
                            state: update.block_id,
                        });
                    } else {
                        session.write_packet(change);
                    }
                }
            }
        }
    }
}
