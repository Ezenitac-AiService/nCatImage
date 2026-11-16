# nCatImage — multi-platform development preview

OpenCode 코딩 에이전트를 통한 AI 기반 개발 프로젝트입니다.

계획된 배포 형식:

- Linux x86_64: AppImage (Ubuntu 24.04/glibc 2.39 이상).
- Windows x86_64: GTK DLL·worker를 포함하는 포터블 ZIP. 단일 portable EXE가 아닙니다.
- macOS Apple Silicon: `.app`을 포함하는 DMG. Intel/Universal binary가 아닙니다.

이 문서는 자동 생성되는 **초안 릴리스**의 검토 템플릿입니다. 게시 전에 해당 태그의 실제 빌드/패키지 smoke 결과·OS 최소 버전·변경 내역·서명 상태를 확인해 수정해야 합니다. Windows/macOS는 현재 포팅 미완료로 사전 검사에서 차단되며 아직 배포 파일이 없습니다.

Windows Authenticode 서명·macOS Developer ID/공증은 미구성입니다. macOS 스크립트의 ad-hoc 서명은 Gatekeeper 신뢰나 Apple 공증이 아닙니다. 체크섬은 서명을 대신하지 않습니다. 실제 desktop·IME·접근성·다양한 GPU 인증은 합성 smoke만으로 완료되지 않습니다.
