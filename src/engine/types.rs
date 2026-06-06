#[derive(Clone, Debug)]
pub enum Score {
    Cp(i32),
    Mate(i32),
}

impl Score {
    pub fn as_cp_f32(&self) -> f32 {
        match self {
            Score::Cp(cp) => *cp as f32,
            Score::Mate(n) => {
                if *n > 0 {
                    100_000.0
                } else {
                    -100_000.0
                }
            }
        }
    }

    pub fn display(&self) -> String {
        match self {
            Score::Cp(cp) => {
                let pawns = *cp as f32 / 100.0;
                if pawns >= 0.0 {
                    format!("+{:.2}", pawns)
                } else {
                    format!("{:.2}", pawns)
                }
            }
            Score::Mate(n) => {
                if *n > 0 {
                    format!("M{}", n)
                } else {
                    format!("-M{}", n.abs())
                }
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct EngineInfo {
    pub depth: u8,
    pub score: Score,
    pub pv: Vec<String>,
    pub multipv: u8,
}

pub enum EngineCmd {
    SetPosition { fen: String, moves: Vec<String> },
    Go { depth: u8, multipv: u8 },
    GoMovetime { ms: u64, multipv: u8 },
    Stop,
    SetOption { name: String, value: String },
}

pub enum EngineOutput {
    Info(EngineInfo),
    BestMove(String),
    Ready,
}
