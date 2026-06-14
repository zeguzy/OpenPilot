## Tasks

- [x] 1. Remove top status bar from render layout
- [x] 2. Add 2-column left gutter (GUTTER constant) for all chat items
- [x] 3. User messages use Gray color, no prefix; AI messages default color, no prefix
- [x] 4. Add is_working/working_start/spinner_frame to App struct
- [x] 5. Implement start_working()/stop_working()/elapsed_secs()/spinner_char() methods
- [x] 6. Add Working status indicator render (spinner + label + elapsed + esc hint)
- [x] 7. Esc key interrupts working state in main.rs run_tui loop
- [x] 8. Spinner animation frame advance on each tick
- [x] 9. Minimal borderless input with Cyan mode-specific prefix
- [x] 10. Add cursor() method to InputArea for cursor positioning
- [x] 11. Add render_footer_info() for model/token/cost display
- [x] 12. All tests pass + 0 clippy warnings
