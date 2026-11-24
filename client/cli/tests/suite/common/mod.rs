//! The test server (`testserver`), and a scripted person at the terminal.

use cli::{Ask, Terminal};
use terms::Secret;

pub(crate) use testserver::Server;

pub(crate) const PASSWORD: &str = "correct horse battery staple";

/// A person who answers what they are asked from a list, and records what was said to them.
#[derive(Default)]
pub(crate) struct Scripted {
    answers: Vec<(Ask, &'static str)>,
    pub said: Vec<String>,
    pub asked: Vec<Ask>,
}

impl Scripted {
    pub(crate) fn answering(answers: &[(Ask, &'static str)]) -> Self {
        Self { answers: answers.to_vec(), ..Self::default() }
    }

    pub(crate) fn heard(&self, part: &str) -> bool {
        self.said.iter().any(|line| line.contains(part))
    }
}

impl Terminal for Scripted {
    fn secret(
        &mut self,
        ask: Ask,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Secret, String>> + Send>> {
        self.asked.push(ask);
        let answer = self
            .answers
            .iter()
            .position(|(a, _)| *a == ask)
            .map(|at| Secret::new(self.answers[at].1.to_string()))
            .ok_or_else(|| format!("asked for {ask:?} unexpectedly"));
        Box::pin(std::future::ready(answer))
    }

    fn say(&mut self, line: &str) {
        self.said.push(line.to_string());
    }

    fn stop(&mut self) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> {
        Box::pin(std::future::pending())
    }
}
