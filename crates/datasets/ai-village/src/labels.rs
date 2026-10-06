//! Label ids: `<world>/<n>`, numbered in the order the converter adds a
//! world's labels (ct-eval's labels had none).

use a2a_bench_format::ids::{InvalidKey, LabelId, WorldKey};

/// The ids of one world's labels.
#[derive(Debug, Clone)]
pub struct LabelIds {
    world: String,
    next: u64,
}

impl LabelIds {
    pub fn new(world: &WorldKey) -> Self {
        Self {
            world: world.as_str().to_owned(),
            next: 0,
        }
    }

    /// The next label's id.
    pub fn take(&mut self) -> Result<LabelId, InvalidKey> {
        let id = LabelId::new(format!("{}/{}", self.world, self.next));
        self.next += 1;
        id
    }
}
