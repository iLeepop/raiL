use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Role {
    User,
    System,
    Assistant,
    Tool,
}
