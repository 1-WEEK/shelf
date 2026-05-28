use std::io::Write;
use std::process::{Command, Stdio};

use crate::{Result, ShelfError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub stdin: Option<Vec<u8>>,
}

impl CommandSpec {
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            stdin: None,
        }
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn stdin(mut self, stdin: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(stdin.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.status == 0
    }
}

pub trait CommandRunner {
    fn run(&mut self, spec: CommandSpec) -> Result<CommandOutput>;
}

#[derive(Debug, Default)]
pub struct SystemRunner;

impl CommandRunner for SystemRunner {
    fn run(&mut self, spec: CommandSpec) -> Result<CommandOutput> {
        let mut command = Command::new(&spec.program);
        command.args(&spec.args);
        if spec.stdin.is_some() {
            command.stdin(Stdio::piped());
        }
        command.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = command.spawn().map_err(|source| ShelfError::CommandIo {
            program: spec.program.clone(),
            source,
        })?;

        if let Some(stdin) = &spec.stdin {
            let mut child_stdin = child.stdin.take().ok_or_else(|| ShelfError::CommandIo {
                program: spec.program.clone(),
                source: std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stdin unavailable"),
            })?;
            child_stdin
                .write_all(stdin)
                .map_err(|source| ShelfError::CommandIo {
                    program: spec.program.clone(),
                    source,
                })?;
        }

        let output = child
            .wait_with_output()
            .map_err(|source| ShelfError::CommandIo {
                program: spec.program.clone(),
                source,
            })?;
        Ok(CommandOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        })
    }
}

pub fn checked(spec: CommandSpec, output: CommandOutput) -> Result<CommandOutput> {
    if output.success() {
        Ok(output)
    } else {
        Err(ShelfError::CommandFailed {
            program: spec.program,
            args: spec.args,
            status: output.status,
            stderr: output.stderr.trim().to_string(),
        })
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;

    #[derive(Default)]
    pub struct MockRunner {
        pub calls: Vec<CommandSpec>,
        pub outputs: Vec<CommandOutput>,
    }

    impl MockRunner {
        pub fn push_success(&mut self, stdout: impl Into<String>) {
            self.outputs.push(CommandOutput {
                status: 0,
                stdout: stdout.into(),
                stderr: String::new(),
            });
        }

        pub fn push_failure(&mut self, stderr: impl Into<String>) {
            self.outputs.push(CommandOutput {
                status: 1,
                stdout: String::new(),
                stderr: stderr.into(),
            });
        }
    }

    impl CommandRunner for MockRunner {
        fn run(&mut self, spec: CommandSpec) -> Result<CommandOutput> {
            self.calls.push(spec);
            Ok(if self.outputs.is_empty() {
                CommandOutput {
                    status: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                }
            } else {
                self.outputs.remove(0)
            })
        }
    }
}
