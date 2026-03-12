# Bootstrap Scripts

## Windows

```powershell
powershell -ExecutionPolicy Bypass -File scripts/bootstrap.ps1
```

처음 세팅부터 직접 할 때:

```powershell
winget install --id Git.Git -e --source winget
winget install Rustlang.Rustup
winget install Microsoft.VisualStudio.2022.BuildTools

git clone <YOUR_REPO_URL>
cd rober
powershell -ExecutionPolicy Bypass -File .\scripts\bootstrap.ps1
```

Git/Rust 설치까지 스크립트에 맡기고 싶다면:

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\bootstrap.ps1 -InstallGit -InstallRust
```

자주 쓰는 옵션:

- `-InstallGit`
- `-InstallRust`
- `-ForceZeroClawInstall`
- `-SourceBuild`
- `-SkipTests`
- `-SkipDoctor`

주의:

- Windows에서 Rust는 보통 `msvc` 타겟을 쓰므로 `cargo test`, `cargo run`에는 Visual Studio Build Tools와 `Desktop development with C++` 워크로드가 필요하다.
- 최신 부트스트랩은 Build Tools가 설치되어 있으면 Visual Studio dev shell을 자동으로 불러오려 시도한다.
- 스크립트는 기본적으로 `zeroclaw` prebuilt binary를 설치하므로, 보통은 `git` 없이도 동작한다.
- `-SourceBuild`를 쓰면 `git`과 소스 빌드 환경이 필요하다.

## macOS / Linux

```bash
bash scripts/bootstrap.sh
```

자주 쓰는 옵션:

- `--force-zeroclaw-install`
- `--source-build`
- `--skip-tests`
- `--skip-doctor`

## What The Scripts Do

1. `zeroclaw` 가용성을 확인한다.
2. 워크스페이스 `cargo test`를 실행한다.
3. `cargo run -p rover-probe -- doctor`를 실행한다.

## Validation Layers

현재 검증은 크게 세 층으로 나뉜다.

| Layer | Entry Point | 핵심 기술 | `zeroclaw` 사용 방식 | 실제로 검증되는 것 |
|---|---|---|---|---|
| Rust 단위 테스트 | `cargo test` | Rust unit test, temp fixture, fake service, fake process runner | 직접 실행하지 않음 | CLI 파싱, fallback 분기, local file/browser adapter 정확성 |
| 순차 마이크로벤치 | `scripts/run-benchmark.ps1` | PowerShell, `rover-probe.exe`, 프로세스 트리 샘플링 | `rover-probe` 내부에서 호환성 확인 후 adapter 선택 | 단건 file/browser step별 latency와 짧은 메모리 샘플 |
| 장시간 스트레스 벤치 | `scripts/run-stress-benchmark.ps1` | PowerShell controller/worker/memory-hog, 목표 시간 기반 loop, worker별 분리 작업 디렉터리 | `rover-probe` 내부에서 호환성 확인 후 adapter 선택 | 장시간 반복, 병렬 worker, 메모리 압박, CPU/메모리/프로세스 수 |

### Technology Notes

- File adapter: `apps/rover-probe/src/local_compat.rs`
  - `list`, `stat`, `open`, `copy`, `move`, `delete`
  - 표준 파일 시스템 API를 직접 사용한다.
- Browser adapter: `apps/rover-probe/src/local_compat.rs`
  - local HTML fixture를 열고 세션 상태 파일을 저장하면서 `open`, `read`, `fill`, `click`, `download`를 흉내 낸다.
- Benchmark sampler: `scripts/benchmark-common.ps1`
  - `Get-Process`, `Win32_Process`, `WorkingSet64`, `PrivateMemorySize64`, `CPU` 값을 사용한다.
  - 자식 프로세스까지 포함한 프로세스 트리를 추적한다.
- Memory pressure: `scripts/run-stress-benchmark.ps1`
  - 별도 PowerShell child process가 큰 byte array를 잡고 주기적으로 touch해서 실제 working set을 유지한다.

## Current `zeroclaw` Status

2026-03-12 기준 현재 설치된 `zeroclaw`는 `tool ...` 서브커맨드와 직접 호환되지 않는다.  
그래서 `rover-probe`는 한 번 호환성을 검사한 뒤, 비호환이면 `local-file` / `local-browser` fallback으로 내려간다.

중요:

- 아래 최신 수치는 `zeroclaw`의 직접적인 tool CLI 성능 수치가 아니다.
- 아래 최신 수치는 현재 배포 환경에서 실제로 동작하는 fallback 경로의 안정성과 자원 사용량을 검증한 결과다.

## Running Validation

### 1. Rust Tests

```powershell
cargo test
```

### 2. Sequential Microbenchmark

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\run-benchmark.ps1
```

자주 쓰는 옵션:

- `-Configuration <debug|release>`
- `-Iterations <n>`
- `-SkipBuild`
- `-SkipFileScenario`
- `-SkipBrowserScenario`
- `-SampleIntervalMs <n>`
- `-OutputPath <path>`

결과 파일:

- `target\benchmarks\<timestamp>\benchmark-results.json`

### 3. Stress Benchmark

```powershell
powershell -ExecutionPolicy Bypass -File .\scripts\run-stress-benchmark.ps1 -Suite all -Difficulty easy
powershell -ExecutionPolicy Bypass -File .\scripts\run-stress-benchmark.ps1 -Suite all -Difficulty medium
powershell -ExecutionPolicy Bypass -File .\scripts\run-stress-benchmark.ps1 -Suite all -Difficulty hard
```

자주 쓰는 옵션:

- `-Suite <single|parallel|all>`
- `-Difficulty <easy|medium|hard>`
- `-Configuration <debug|release>`
- `-SkipBuild`
- `-SkipFileScenario`
- `-SkipBrowserScenario`
- `-SampleIntervalMs <n>`
- `-OutputPath <path>`

결과 파일:

- `target\stress-benchmarks\<timestamp>\stress-benchmark-results.json`

## Metrics In Result JSON

기본 측정 필드:

- `duration_ms`
- `peak_working_set_mb`
- `peak_private_mb`
- `peak_cpu_percent`
- `peak_process_count`
- `sample_count`
- `exit_code`
- `stdout_path`
- `stderr_path`

스트레스 전용 필드:

- `suite`
- `difficulty`
- `resolved_adapter`
- `worker_id`
- `target_duration_ms`
- `payload_size_bytes`
- `memory_pressure_mb`
- `background_process_count`
- `loop_iterations`
- `final_result`

CPU 해석 규칙:

- `peak_cpu_percent` 는 전체 논리 코어 합산 기준이다.
- 대략 `100% = 논리 코어 1개를 꽉 쓰는 수준`으로 보면 된다.

## Latest Validated Snapshot

검증 일시: 2026-03-12  
검증 명령:

```powershell
cargo test
powershell -ExecutionPolicy Bypass -File .\scripts\run-benchmark.ps1 -Configuration release
powershell -ExecutionPolicy Bypass -File .\scripts\run-stress-benchmark.ps1 -Suite all -Difficulty easy -Configuration release -SkipBuild
powershell -ExecutionPolicy Bypass -File .\scripts\run-stress-benchmark.ps1 -Suite all -Difficulty medium -Configuration release -SkipBuild
powershell -ExecutionPolicy Bypass -File .\scripts\run-stress-benchmark.ps1 -Suite all -Difficulty hard -Configuration release -SkipBuild
```

결과 요약:

- `cargo test`: `22/22` 통과
- 마이크로벤치: `30/30` 성공
- 스트레스 벤치: `24/24` worker run 성공
- 실제 adapter: `local-file`, `local-browser`

### Sequential Microbenchmark

결과 파일:

- `target\benchmarks\20260312-184235\benchmark-results.json`

| Scenario | Step | Avg Duration (ms) | Avg Peak Memory (MB) | Avg Peak CPU (%) | 해석 |
|---|---|---:|---:|---:|---|
| file | `stat-seed` | 89.67 | 0.00 | 0.00 | 가장 느린 file 단건 step |
| file | `copy-seed` | 45.67 | 0.00 | 0.00 | 가벼운 복사 |
| file | `move-copy` | 50.67 | 0.00 | 0.00 | 가벼운 rename |
| file | `delete-moved` | 46.00 | 0.00 | 0.00 | 가벼운 삭제 |
| browser | `open-fixture` | 50.67 | 1.44 | 0.00 | fixture open |
| browser | `read-body` | 165.67 | 199.04 | 0.00 | microbenchmark 기준 가장 무거운 browser step |
| browser | `fill-name` | 48.33 | 0.00 | 0.00 | 짧은 입력 |
| browser | `fill-notes` | 46.67 | 0.00 | 0.00 | 짧은 입력 |
| browser | `click-submit` | 48.00 | 0.00 | 0.00 | state update |
| browser | `read-result` | 49.00 | 0.00 | 0.00 | 결과 읽기 |

참고:

- 이 러너는 기본 샘플 간격이 `200ms`라서, 짧게 끝나는 step은 CPU가 `0`으로 보일 수 있다.
- CPU/메모리 해석은 아래 stress 벤치 결과가 훨씬 신뢰도가 높다.

### Stress Benchmark

결과 파일:

- `target\stress-benchmarks\20260312-184245\stress-benchmark-results.json`
- `target\stress-benchmarks\20260312-184419\stress-benchmark-results.json`
- `target\stress-benchmarks\20260312-184647\stress-benchmark-results.json`

| Difficulty | Scenario | Suite | Avg Duration (ms) | Avg Peak Memory (MB) | Avg Peak CPU (%) | Min Samples | Adapter | 해석 |
|---|---|---|---:|---:|---:|---:|---|---|
| easy | file | single | 17886 | 240.75 | 117.19 | 104 | `local-file` | 단건 백그라운드 실행 가능 |
| easy | file | parallel | 24483 | 718.53 | 492.19 | 102 | `local-file` | 눈에 띄는 CPU 사용 |
| easy | browser | single | 17382 | 202.95 | 125.00 | 97 | `local-browser` | 단건 백그라운드 실행 가능 |
| easy | browser | parallel | 24686 | 669.18 | 609.38 | 100 | `local-browser` | CPU 영향 큼 |
| medium | file | single | 33595 | 854.93 | 101.56 | 204 | `local-file` | 메모리 부담이 커짐 |
| medium | file | parallel | 35091 | 2860.96 | 816.41 | 116 | `local-file` | 적극 업무 병행 비권장 |
| medium | browser | single | 33097 | 197.16 | 148.44 | 172 | `local-browser` | 브라우저 단건은 여전히 무난 |
| medium | browser | parallel | 35279 | 1011.10 | 886.72 | 107 | `local-browser` | CPU/메모리 모두 체감 가능 |
| hard | file | single | 66002 | 2242.07 | 117.19 | 413 | `local-file` | 장시간, 메모리 큼 |
| hard | file | parallel | 55720 | 7254.77 | 1011.72 | 155 | `local-file` | 사실상 전용 리소스 필요 |
| hard | browser | single | 62557 | 203.22 | 121.09 | 355 | `local-browser` | CPU는 낮지만 오래 돈다 |
| hard | browser | parallel | 52036 | 2156.76 | 1613.28 | 92 | `local-browser` | 코어 경쟁이 매우 큼 |

### Parallelism / Pressure Snapshot

| Difficulty | Scenario | Suite | Min Peak Process Count | Max Loop Iterations | 비고 |
|---|---|---|---:|---:|---|
| easy | file | parallel | 4 | 127 | worker 2 + controller + sampler 계층 확인 |
| easy | browser | parallel | 5 | 104 | worker별 세션 분리 정상 |
| medium | file | parallel | 6 | 48 | 메모리 압박 512MB 반영 |
| medium | browser | parallel | 5 | 125 | 병렬 브라우저 세션 충돌 없음 |
| hard | file | parallel | 9 | 18 | 메모리 압박 1024MB + 대용량 file payload |
| hard | browser | parallel | 14 | 93 | 가장 공격적인 CPU 경쟁 구간 |

브라우저 결과 무결성:

- `hard`까지 `final_result = Submitted:worker-<n>-iteration-<m>|notes-...` 형식으로 검증됐다.
- worker별 전용 작업 디렉터리를 써서 세션 충돌은 재현되지 않았다.

## Performance Insight: Can This Run While A User Is Working?

핵심 질문은 “사용자가 백그라운드에서 업무를 보는 동안 병렬로 돌려도 되느냐”인데, 현재 데이터 기준 결론은 아래와 같다.

| 사용성 판단 | 추천 범위 | 이유 |
|---|---|---|
| 가능 | `browser single easy`, `browser single medium`, `browser single hard`, `file single easy` | 메모리 약 `200MB ~ 240MB`, CPU 약 `1.0 ~ 1.5` 논리 코어 수준이라 일반 개발/문서 작업과 병행 가능 |
| 조건부 가능 | `file single medium`, `file parallel easy`, `browser parallel easy` | 체감 부하는 분명 있다. 특히 parallel easy는 CPU가 `4.9 ~ 6.1` 논리 코어 수준이라 회의, 빌드, 화상 통화와는 충돌 가능 |
| 비권장 | `file parallel medium`, `browser parallel medium`, `file single hard`, `file parallel hard`, `browser parallel hard` | 메모리 `1GB ~ 7.25GB`, CPU `8 ~ 16` 논리 코어 수준으로 올라가서 적극적인 업무 병행 시 지연이 커질 가능성이 높다 |

### Practical Recommendation

- 업무 중 상시 백그라운드 검증:
  - `single + easy`
  - 브라우저 위주면 `single + medium`까지 가능
- 점심시간/유휴시간 검증:
  - `parallel + easy`
  - `single + hard`
- 사용자가 IDE, 브라우저, 화상회의, 빌드를 동시에 쓰는 시간대에는 피할 것:
  - `parallel + medium`
  - `parallel + hard`

### Why This Conclusion Matters

- 현재 병렬 테스트는 “실패만 안 나는지” 수준이 아니라, 실제로 메모리 압박 프로세스와 여러 worker를 함께 띄워서 자원 경쟁을 만들고 있다.
- 그래서 `hard parallel` 수치는 개발자가 체감하는 UI 끊김, 빌드 지연, 브라우저 탭 재로딩 가능성과 더 직접적으로 연결된다.
- 특히 `file parallel hard`의 평균 peak memory `7.25GB`와 `browser parallel hard`의 평균 peak CPU `1613%`는, 일반 노트북에서는 업무 병행용이 아니라 전용 검증 시간대로 보는 편이 안전하다.

## Known Limits

- 현재 수치는 direct `zeroclaw tool ...` CLI 수치가 아니라 fallback adapter 수치다.
- 마이크로벤치의 CPU 수치는 short-lived process 특성상 과소측정될 수 있다.
- `cargo test` 도중 Rust incremental artifact 경고가 드물게 뜰 수 있지만, 이번 검증에서는 자동 복구 후 정상 통과했다.
