use egui::{Pos2, Vec2};

use crate::TileId;

#[cfg_attr(feature = "serde", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Debug, PartialEq)]
pub struct FloatingWindow {
    pub root: TileId,
    pub pos: Pos2,
    pub size: Vec2,
    pub z: u64,
}

impl FloatingWindow {
    pub fn new(root: TileId, pos: Pos2, size: Vec2, z: u64) -> Self {
        Self { root, pos, size, z }
    }
}

