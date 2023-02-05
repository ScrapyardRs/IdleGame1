use std::path::{Path, PathBuf};

use drax::prelude::Uuid;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::game::grip_item::GripItem;
use crate::ranks::Rank;

#[derive(serde_derive::Serialize, serde_derive::Deserialize, Debug, Clone)]
pub struct PlayerDbInformation {
    pub uuid: Uuid,
    pub name: String,
    pub rank: Rank,
    pub block_data: crate::game::blocks::PlayerBlockData,
    pub grip_item: GripItem,
}

const DB_PATH: &'static str = "/home/minecraft/server/db";
const PLAYER_DB_EXT: &'static str = "players";

pub fn ensure_db() {
    let db_path = Path::new(DB_PATH);
    if !db_path.exists() {
        std::fs::create_dir_all(db_path).unwrap();
    }
    let player_db_path = db_path.join(PLAYER_DB_EXT);
    if !player_db_path.exists() {
        std::fs::create_dir_all(player_db_path).unwrap();
    }
}

pub struct DbHook<T> {
    pub hook_path: PathBuf,
    _phantom_t: std::marker::PhantomData<T>,
}

impl DbHook<()> {
    pub fn player(id: Uuid) -> DbHook<PlayerDbInformation> {
        let db_path = Path::new(DB_PATH);
        let player_db_path = db_path.join(PLAYER_DB_EXT);
        DbHook {
            hook_path: player_db_path.join(id.to_string()),
            _phantom_t: Default::default(),
        }
    }
}

impl<T> DbHook<T> {
    pub fn insert(&self, data: &T) -> serde_json::Result<()>
    where
        T: Serialize,
    {
        let mut file = std::fs::File::create(&self.hook_path).unwrap();
        serde_json::to_writer_pretty(&mut file, data)?;
        Ok(())
    }

    pub fn load(&self) -> serde_json::Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        if !self.hook_path.exists() {
            return Ok(None);
        }
        let mut file = std::fs::File::open(&self.hook_path).unwrap();
        let data = serde_json::from_reader(&mut file)?;
        Ok(data)
    }
}
