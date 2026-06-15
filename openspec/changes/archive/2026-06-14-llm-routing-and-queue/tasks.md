## Tasks

- [x] 1. Rewrite orchestrator prompt with OPCA_DISPATCH instruction
- [x] 2. Remove route() call from RealOrchestrator::handle_message
- [x] 3. Add stream_foreground with first-line buffer for prefix detection
- [x] 4. Flush buffer on newline or Done for non-dispatch responses
- [x] 5. Detect OPCA_DISPATCH: prefix after stream completion
- [x] 6. Send StreamEvent::Dispatch with extracted description
- [x] 7. Remove duplicate dispatch_task spawn from stream_foreground
- [x] 8. poll_stream Dispatch handler calls orchestrator.dispatch()
- [x] 9. Add pending_messages queue to App
- [x] 10. Enter during is_working enqueues message
- [x] 11. Tick auto-sends queued messages when is_working false
- [x] 12. Render queue indicator (N queued - next: preview)
- [x] 13. dispatch error shown in TUI instead of silent
- [x] 14. Chinese routing keywords added (kept for fallback, not used by real.rs)
- [x] 15. All tests pass + 0 clippy warnings
