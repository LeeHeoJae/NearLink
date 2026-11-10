# 배터리 위젯 진단 로그

NearLink의 배터리 송신·수신 경로와 삼성 위젯을 비교할 때 사용합니다. 로그는 앱 프로세스의 Logcat에 기록되며 별도로 파일에 저장되거나 전송되지 않습니다. 기기 모델, Android 및 One UI 버전, 화면 상태와 실제 배터리·앱·위젯의 값을 시험 시각과 함께 기록하세요.

## 수집

두 기기를 USB 디버깅으로 연결하고 `adb devices`에서 시리얼을 확인합니다. PowerShell 창을 기기마다 하나씩 열어 다음 명령을 실행한 후 앱을 연결하고 문제를 재현합니다. 창을 닫거나 Ctrl+C로 수집을 종료합니다.

```powershell
adb -s <보내는_기기_시리얼> logcat -v threadtime -s BatteryBleProbe:I BatteryBleReceiver:I > sender-log.txt
```

```powershell
adb -s <삼성_기기_시리얼> logcat -v threadtime -s BatteryBleProbe:I BatteryBleReceiver:I > samsung-log.txt
```

양쪽 기기가 배터리를 주고받기 때문에 각 로그에 송신과 수신 태그가 모두 나타날 수 있습니다. Logcat은 실행 전에 남은 로그도 출력할 수 있으므로 재현 시각을 기준으로 구분하세요. 각 기기의 시간이 맞는지 확인하면 비교가 쉽습니다. 로그 버퍼는 순환하므로 장시간 시험은 수집을 켜 둔 채 진행하세요.

## 읽는 방법

- `BatteryBleProbe`: 로컬 배터리 변경(`battery changed`), 읽기 요청과 응답(`read`), 구독 요청(`descriptor write`), 알림 대기열·발송 요청·콜백(`notification enqueue`, `request`, `callback`), 시간 초과와 GATT 서버 시작·종료를 기록합니다. `generation`은 서버 재시작을 구분합니다. `peer`는 송신 서비스가 실행되는 동안에만 유효한 임시 번호이며 Bluetooth 주소가 아닙니다. 서비스가 다시 시작되면 `generation`과 `peer` 번호도 초기화됩니다.
- `BatteryBleReceiver`: NearLink가 상대방에게 요청한 읽기와 구독, 그리고 읽기 또는 알림으로 받은 값(`value source=read` 또는 `notification`)과 파싱 결과를 기록합니다.
- 특성 `00002a19-...`는 배터리 잔량, `00002bed-...`는 배터리 상태입니다. `value`는 전송된 바이트의 16진수이며 `status`, `result`, `accepted`는 각 단계의 결과입니다. 알림 요청이 수락되거나 콜백이 성공해도 삼성 위젯의 화면 반영까지 확인되는 것은 아닙니다.
- 송신 로그의 `peer`는 원격 기기를 나타냅니다. 같은 원격 기기에서 NearLink와 삼성 시스템 중 어느 구성요소가 요청했는지는 이 번호만으로 구분할 수 없습니다.

로그에는 배터리 상태와 기기 사용 시각 등이 포함됩니다. 공유 전에 내용을 확인하세요. 위젯에 실제로 표시된 값은 앱 로그에 기록되지 않으므로 별도로 메모하거나 화면을 캡처해야 합니다.
