use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, Sender},
    thread,
};

use super::types::{EngineCmd, EngineInfo, EngineOutput, Score};

pub struct UciClient {
    tx: Sender<EngineCmd>,
    rx: Receiver<EngineOutput>,
    _child: Option<Child>,
}

impl UciClient {
    pub fn new(path: &Path) -> anyhow::Result<Self> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;

        let mut stdin = child.stdin.take().expect("piped stdin");
        let stdout = child.stdout.take().expect("piped stdout");

        let (tx_cmd, rx_cmd) = mpsc::channel::<EngineCmd>();
        let (tx_out, rx_out) = mpsc::channel::<EngineOutput>();

        // Writer thread: convert EngineCmd -> UCI text lines
        thread::spawn(move || {
            for cmd in rx_cmd {
                let line = match cmd {
                    EngineCmd::SetPosition { fen, moves } => {
                        let pos_str = if fen == "startpos" {
                            "startpos".to_string()
                        } else {
                            format!("fen {}", fen)
                        };
                        if moves.is_empty() {
                            format!("position {}", pos_str)
                        } else {
                            format!("position {} moves {}", pos_str, moves.join(" "))
                        }
                    }
                    EngineCmd::Go { depth, multipv } => {
                        format!(
                            "setoption name MultiPV value {}\ngo depth {}",
                            multipv, depth
                        )
                    }
                    EngineCmd::GoMovetime { ms, multipv } => {
                        format!(
                            "setoption name MultiPV value {}\ngo movetime {}",
                            multipv, ms
                        )
                    }
                    EngineCmd::Stop => "stop".to_string(),
                    EngineCmd::SetOption { name, value } => {
                        format!("setoption name {} value {}", name, value)
                    }
                };
                for l in line.lines() {
                    if writeln!(stdin, "{}", l).is_err() {
                        return;
                    }
                }
                let _ = stdin.flush();
            }
        });

        // Reader thread: parse UCI output lines
        let tx_out_clone = tx_out.clone();
        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines() {
                let Ok(line) = line else { break };
                if line == "readyok" {
                    let _ = tx_out_clone.send(EngineOutput::Ready);
                } else if let Some(rest) = line.strip_prefix("bestmove ") {
                    let best = rest.split_whitespace().next().unwrap_or("").to_string();
                    if !best.is_empty() && best != "(none)" {
                        let _ = tx_out_clone.send(EngineOutput::BestMove(best));
                    }
                } else if line.starts_with("info ") {
                    if let Some(info) = parse_info(&line) {
                        let _ = tx_out_clone.send(EngineOutput::Info(info));
                    }
                }
            }
        });

        // Send UCI init
        tx_cmd.send(EngineCmd::SetOption {
            name: "UCI_AnalyseMode".to_string(),
            value: "true".to_string(),
        })?;

        Ok(Self {
            tx: tx_cmd,
            rx: rx_out,
            _child: Some(child),
        })
    }

    pub fn send(&self, cmd: EngineCmd) {
        let _ = self.tx.send(cmd);
    }

    pub fn try_recv(&self) -> Option<EngineOutput> {
        self.rx.try_recv().ok()
    }

    pub fn set_position(&self, fen: &str, moves: &[String]) {
        self.send(EngineCmd::SetPosition {
            fen: fen.to_string(),
            moves: moves.to_vec(),
        });
    }

    pub fn go_depth(&self, depth: u8, multipv: u8) {
        self.send(EngineCmd::Go { depth, multipv });
    }

    pub fn go_movetime(&self, ms: u64) {
        self.send(EngineCmd::GoMovetime { ms, multipv: 1 });
    }

    pub fn stop(&self) {
        self.send(EngineCmd::Stop);
    }

    pub fn set_elo(&self, elo: u32) {
        self.send(EngineCmd::SetOption {
            name: "UCI_LimitStrength".to_string(),
            value: "true".to_string(),
        });
        self.send(EngineCmd::SetOption {
            name: "UCI_Elo".to_string(),
            value: elo.to_string(),
        });
    }

    pub fn disable_limit_strength(&self) {
        self.send(EngineCmd::SetOption {
            name: "UCI_LimitStrength".to_string(),
            value: "false".to_string(),
        });
    }
}

fn parse_info(line: &str) -> Option<EngineInfo> {
    let tokens: Vec<&str> = line.split_whitespace().collect();
    if tokens.first() != Some(&"info") {
        return None;
    }

    let mut depth = 0u8;
    let mut score: Option<Score> = None;
    let mut pv: Vec<String> = vec![];
    let mut multipv = 1u8;

    let mut i = 1;
    while i < tokens.len() {
        match tokens[i] {
            "depth" => {
                depth = tokens.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(0);
                i += 2;
            }
            "multipv" => {
                multipv = tokens.get(i + 1).and_then(|s| s.parse().ok()).unwrap_or(1);
                i += 2;
            }
            "score" => {
                match tokens.get(i + 1) {
                    Some(&"cp") => {
                        score = tokens
                            .get(i + 2)
                            .and_then(|s| s.parse().ok())
                            .map(Score::Cp);
                        i += 3;
                    }
                    Some(&"mate") => {
                        score = tokens
                            .get(i + 2)
                            .and_then(|s| s.parse().ok())
                            .map(Score::Mate);
                        i += 3;
                    }
                    _ => {
                        i += 1;
                    }
                }
                // skip "lowerbound" / "upperbound" if present
                if let Some(&"lowerbound") | Some(&"upperbound") = tokens.get(i) {
                    i += 1;
                }
            }
            "pv" => {
                i += 1;
                while i < tokens.len() {
                    pv.push(tokens[i].to_string());
                    i += 1;
                }
            }
            _ => {
                i += 1;
            }
        }
    }

    Some(EngineInfo {
        depth,
        score: score.unwrap_or(Score::Cp(0)),
        pv,
        multipv,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_info_line() {
        let line = "info depth 20 seldepth 28 multipv 1 score cp 31 nodes 1000 nps 50000 \
                    time 20 pv e2e4 e7e5 g1f3";
        let info = parse_info(line).expect("should parse");
        assert_eq!(info.depth, 20);
        assert_eq!(info.multipv, 1);
        assert!(matches!(info.score, Score::Cp(31)));
        assert_eq!(info.pv, vec!["e2e4", "e7e5", "g1f3"]);
    }

    #[test]
    fn parses_mate_score() {
        let info = parse_info("info depth 10 score mate 3 pv e2e4").expect("should parse");
        assert!(matches!(info.score, Score::Mate(3)));
    }

    #[test]
    fn skips_bound_token_without_corrupting_pv() {
        let info =
            parse_info("info depth 12 score cp 45 upperbound pv e2e4 e7e5").expect("should parse");
        assert!(matches!(info.score, Score::Cp(45)));
        assert_eq!(info.pv, vec!["e2e4", "e7e5"]);
    }

    #[test]
    fn returns_none_for_non_info_line() {
        assert!(parse_info("bestmove e2e4 ponder e7e5").is_none());
        assert!(parse_info("readyok").is_none());
    }

    #[test]
    fn missing_score_defaults_to_zero_cp() {
        let info = parse_info("info depth 5 pv e2e4").expect("should parse");
        assert!(matches!(info.score, Score::Cp(0)));
    }
}
