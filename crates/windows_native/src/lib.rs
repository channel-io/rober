use rover_core::{NativeRequest, OutputValue, ProbeAdapter, ProbeResult, Status};

#[derive(Default)]
pub struct NativeAdapter;

impl ProbeAdapter<NativeRequest> for NativeAdapter {
    fn adapter_name(&self) -> &'static str {
        "windows-native"
    }

    fn run(&self, request: NativeRequest) -> Result<ProbeResult, rover_core::ProbeError> {
        let action = request.action_name().to_string();
        let platform = if cfg!(windows) { "windows" } else { "non-windows" };

        Ok(ProbeResult::with_output(
            self.adapter_name(),
            action,
            Status::NotImplemented,
            0,
            "native Windows adapter is a reserved extension point and is not implemented yet",
            OutputValue::object(vec![
                ("platform", OutputValue::string(platform)),
                ("available", OutputValue::Bool(false)),
            ]),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rover_core::NativeRequest;

    #[test]
    fn returns_not_implemented_stub() {
        let adapter = NativeAdapter;
        let result = adapter
            .run(NativeRequest::Inspect {
                target: Some("Notepad".into()),
            })
            .unwrap();

        assert_eq!(result.status, Status::NotImplemented);
        assert!(result.summary.contains("not implemented"));
    }
}
