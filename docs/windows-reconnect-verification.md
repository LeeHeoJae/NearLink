# Windows 재실행 후 재연결 검증 · 2026-11-15

## 확인한 문제와 변경

기존 실행 중인 앱에서 저장된 Android 기기 두 대가 배터리 값을 받지 못하고
재연결 대기를 반복했다. 화면의 오류는 배터리 서비스 검색 분기에서 발생했지만,
기존 구현이 GATT 통신 실패와 서비스 부재를 같은 안내문으로 바꾸어 세부 상태를
확인할 수 없었다.

- 연결 시도마다 `GattSession`을 소유하고 `MaintainConnection`을 활성화한다.
  실패·중지·종료 시 콜백, 서비스, 유지 요청과 세션을 해제한다.
- 앱 종료 시 모든 작업을 먼저 취소하고 종료를 기다린 뒤 호스트 창과 단일 실행
  잠금을 해제한다. 종료 직전에 학습한 페어링 식별자도 저장한다.
- 서비스 검색 오류에 Windows의 Unreachable, AccessDenied, ProtocolError를
  구분해서 표시하고, 검색은 성공했지만 서비스가 없는 경우도 구분한다.

Microsoft의 [GATT 클라이언트 문서](https://learn.microsoft.com/en-us/windows/apps/develop/devices-sensors/gatt-client)와
[MaintainConnection 문서](https://learn.microsoft.com/en-us/uwp/api/windows.devices.bluetooth.genericattributeprofile.gattsession.maintainconnection)를 참고했다.
장치 객체 생성만으로 연결 유지가 보장되지는 않으며, 세션의 유지 요청은 장치가
사용 가능해졌을 때 Windows가 연결하도록 한다. 최초 장애의 Windows 상태는
기존 오류 처리 때문에 보존되지 않았으므로 단일 원인으로 단정하지 않는다.

## 검증 범위

- `windows/build.ps1 test`: 14개 성공, 수동 페어링 대화 상자 테스트 1개 제외.
- `windows/build.ps1 check`: 경고를 오류로 처리하는 Clippy 검사 성공.
- 릴리스 빌드 성공.
- 실제 사용자 프로필을 유지한 채 기존 앱을 수정 빌드로 교체해 실행했다.
  저장된 두 Android 기기가 사용자 재등록이나 추가 페어링 없이 연결되었고,
  새 배터리 데이터 수신과 연결됨 상태를 확인했다.
- 수정 앱을 다시 종료·재실행한 뒤에도 두 기기의 자동 연결과 새 잔량 수신을
  확인했다. 이후 30초 세션 확인 주기가 지난 시점에도 두 기기가 연결 상태를 유지했다.
- 모든 작업을 먼저 취소한 뒤 합류하는 종료 순서와 반복 종료 호출은 자동 테스트로
  확인했다. 실제 앱의 반복 재실행 검사는 강제 프로세스 종료를 포함하므로,
  트레이 메뉴의 정상 종료 경로와는 구분한다.

절전 복귀, Windows 재부팅, Android 서비스 재시작, 장시간 재연결은 이번 검증에
포함하지 않는다. 변경 전 실행 파일은 로컬 검증 폴더에 백업했다.
