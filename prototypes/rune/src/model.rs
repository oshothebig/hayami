use rune_kit::diff::LineRange;

pub struct Comment {
    pub id: usize,
    pub range: LineRange,
    pub body: String,
}

#[derive(Default)]
pub struct Comments {
    pub composing: Option<LineRange>,
    pub posted: Vec<Comment>,
}

impl Comments {
    pub fn post(&mut self, body: &str) -> bool {
        let Some(range) = self.composing else {
            return false;
        };
        if body.trim().is_empty() {
            return false;
        }
        self.posted.push(Comment {
            id: self.posted.len(),
            range,
            body: body.trim().into(),
        });
        self.composing = None;
        true
    }
}
