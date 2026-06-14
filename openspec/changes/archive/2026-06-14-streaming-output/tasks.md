## Tasks

- [x] 1. Add StreamEvent enum (Delta/Done/Error) to app.rs
- [x] 2. Add StreamingAssistant variant to ChatItem
- [x] 3. Add stream_rx/stream_tx to App struct
- [x] 4. Add poll_stream() method to drain channel and update StreamingAssistant
- [x] 5. Add stream_foreground to OrchestratorApi trait
- [x] 6. Implement stream_foreground in RealOrchestrator (spawn async provider stream)
- [x] 7. Implement stream_foreground in MockOrchestrator (word-by-word echo)
- [x] 8. Remove query_llm call from handle_message (no blocking)
- [x] 9. Task status queries return Acknowledged instead of Foreground
- [x] 10. Render StreamingAssistant same as AssistantText
- [x] 11. run_tui: call stream_foreground on Foreground reply + poll_stream on tick
- [x] 12. All tests pass + 0 clippy warnings
