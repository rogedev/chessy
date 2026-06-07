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

    pub fn negated(&self) -> Score {
        match self {
            Score::Cp(cp) => Score::Cp(-cp),
            Score::Mate(n) => Score::Mate(-n),
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

impl EngineInfo {
    /// An empty line for the given MultiPV slot, used to pad gaps before a
    /// higher-numbered line arrives from the engine.
    pub fn placeholder(multipv: u8) -> Self {
        Self {
            depth: 0,
            score: Score::Cp(0),
            pv: vec![],
            multipv,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_cp_formats_with_sign_and_two_decimals() {
        assert_eq!(Score::Cp(123).display(), "+1.23");
        assert_eq!(Score::Cp(-50).display(), "-0.50");
        assert_eq!(Score::Cp(0).display(), "+0.00");
    }

    #[test]
    fn display_mate_uses_m_notation() {
        assert_eq!(Score::Mate(3).display(), "M3");
        assert_eq!(Score::Mate(-2).display(), "-M2");
    }

    #[test]
    fn negated_flips_sign_for_cp_and_mate() {
        assert!(matches!(Score::Cp(150).negated(), Score::Cp(-150)));
        assert!(matches!(Score::Cp(-50).negated(), Score::Cp(50)));
        assert!(matches!(Score::Mate(8).negated(), Score::Mate(-8)));
        assert!(matches!(Score::Mate(-3).negated(), Score::Mate(3)));
    }

    #[test]
    fn as_cp_f32_returns_centipawns_for_cp() {
        assert_eq!(Score::Cp(150).as_cp_f32(), 150.0);
        assert_eq!(Score::Cp(-150).as_cp_f32(), -150.0);
    }

    #[test]
    fn as_cp_f32_clamps_mate_to_large_magnitude() {
        assert_eq!(Score::Mate(1).as_cp_f32(), 100_000.0);
        assert_eq!(Score::Mate(-1).as_cp_f32(), -100_000.0);
    }
}
