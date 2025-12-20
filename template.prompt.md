# Vibecoding Prompt Template

## Goal
{{GOAL}}

## Product Context
시음노트 서비스 서버

## Tech Stack
- rust [Actix](https://actix.rs/)
- 인증: Auth0
- ORM: [Diesel](https://github.com/diesel-rs/diesel)
    - [예제](https://github.com/actix/examples/tree/master/databases/diesel)
- DB postgreSQL
- static image server는 actix_files::Files을 이용하여 구현

## Architecture Rules (MUST)
- 요청 해더와 응답값은 다음과 같은 공통 포멧을 따름
    - common header
    ```json
    {"authorization": "Bearer {ACCESS_TOKEN}"}
    ```
    - common response model
    ```json
    // success
    {
        "result": true,
        "data": data model
    }

    // fail
    {
        "result": false,
        "error": 100
    }
    ```
- error에 관련된 선언은 src/errors.rs 참고
- handler 구현은 src/user_handler.rs 참고
- test 구현은 tests/user_test.rs 참고

## Constraints & Preferences
- 서드파티 추가 금지
- 설명은 한글로 출력
- 다음 파일은 수정 금지
    - src/schema.rs

## Acceptance Criteria
- [작업별로 Codex가 제안/보완해도 됨. 필요하면 Goal/Scope에 포함해도 OK]
