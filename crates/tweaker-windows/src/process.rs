//! Process runner on top of `std::process`.

use std::process::Command;

use tweaker_core::adapters::{AdapterError, IProcessRunner, ProcessOutput};

/// Runs external programs and captures their output.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemProcessRunner;

impl IProcessRunner for SystemProcessRunner {
    fn run(&self, program: &str, args: &[String]) -> Result<ProcessOutput, AdapterError> {
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|error| AdapterError::new(format!("cannot run {program}: {error}")))?;
        Ok(ProcessOutput {
            code: output.status.code(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_cmd_and_captures_stdout() {
        let runner = SystemProcessRunner;
        let output = runner
            .run("cmd.exe", &["/C".to_owned(), "echo wintweaker".to_owned()])
            .expect("run");
        assert_eq!(output.code, Some(0));
        assert!(output.stdout.contains("wintweaker"));
    }

    #[test]
    fn missing_program_is_an_error() {
        let runner = SystemProcessRunner;
        let error = runner
            .run("wintweaker-no-such-program.exe", &[])
            .expect_err("must fail");
        assert!(!error.message.is_empty());
    }
}
