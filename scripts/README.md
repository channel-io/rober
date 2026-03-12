# Bootstrap scripts

## Windows

```powershell
powershell -ExecutionPolicy Bypass -File scripts/bootstrap.ps1
```

처음 세팅할 때 가장 단순한 순서:

```powershell
winget install --id Git.Git -e --source winget
winget install Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools

git clone <YOUR_REPO_URL>
cd rober
powershell -ExecutionPolicy Bypass -File .\scripts\bootstrap.ps1
```

Git과 Rust를 스크립트가 대신 설치하게 하려면:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\bootstrap.ps1 -InstallGit -InstallRust
```

주의:

- Windows에서 현재 Rust 타깃은 `msvc` 기준이므로, `cargo test`와 `cargo run`을 하려면 Visual Studio Build Tools와 `Desktop development with C++` 워크로드가 필요
- 최신 스크립트는 Build Tools가 설치된 경우 Visual Studio dev shell을 자동으로 불러오려 시도
- 위 스크립트는 기본적으로 `zeroclaw` prebuilt binary를 설치하므로, 보통은 `git` 없이도 동작
- `-SourceBuild`를 사용할 때는 `git`이 반드시 필요
- Build Tools 설치 후에는 PowerShell 창을 새로 열고 다시 실행하는 편이 안전

Useful flags:

- `-InstallGit`
- `-InstallRust`
- `-ForceZeroClawInstall`
- `-SourceBuild`
- `-SkipTests`
- `-SkipDoctor`

## macOS / Linux

```bash
bash scripts/bootstrap.sh
```

Useful flags:

- `--force-zeroclaw-install`
- `--source-build`
- `--skip-tests`
- `--skip-doctor`

## What the scripts do

1. Ensure `zeroclaw` is available.
2. Run `cargo test` for this workspace.
3. Run `cargo run -p rover-probe -- doctor`.

## Windows benchmark runner

PowerShell에서 `LLM 없이` file/browser 제어 경로를 여러 번 돌리고, step별 지표를 남기는 스크립트:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\run-benchmark.ps1
```

기본 지표:

- `duration_ms`
- `peak_working_set_mb`
- `peak_private_mb`
- `peak_cpu_percent`
- `exit_code`
- `stdout/stderr` 로그 경로

옵션 예시:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\run-benchmark.ps1 -Iterations 5
powershell -ExecutionPolicy Bypass -File .\scripts\run-benchmark.ps1 -SkipBrowserScenario
powershell -ExecutionPolicy Bypass -File .\scripts\run-benchmark.ps1 -SkipFileScenario
```

결과는 기본적으로 `target\benchmarks\<timestamp>\benchmark-results.json`에 저장됩니다.

참고:

- browser 시나리오는 `zeroclaw`가 브라우저 세션을 step 사이에서 유지한다는 가정을 둠
- 측정치는 `rover-probe`와 그 step 실행 중에 직접 딸린 자식 프로세스를 기준으로 계산됨
