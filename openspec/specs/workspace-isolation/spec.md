## ADDED Requirements

### Requirement: Workspace trait abstracts isolation
The system SHALL provide a Workspace trait that abstracts task workspace creation, freezing, diffing, merging, and cleanup. Implementations include GitWorkspace (git projects), MirrorWorkspace (non-git via internal git mirror), and CopyWorkspace (fallback full copy).

#### Scenario: Create workspace for git project
- **WHEN** a Task is dispatched in a git project
- **THEN** a GitWorkspace is created via `git worktree add` in `.agent/worktrees/<task-id>/`
- **AND** the workspace path is a real filesystem directory where bash/cargo/npm work normally

#### Scenario: Create workspace for non-git project
- **WHEN** a Task is dispatched in a non-git project
- **THEN** a MirrorWorkspace is created by initializing an internal git repo in `.agent/mirror/`, importing project files, then creating a worktree from the mirror baseline

### Requirement: Workspace freeze makes it read-only
When a Task completes (Delivered state), its workspace SHALL be frozen—no further write operations are allowed. This ensures the diff is stable for audit and merge.

#### Scenario: Write after freeze fails
- **WHEN** a workspace is frozen and a write tool is invoked
- **THEN** the write is rejected with a "workspace frozen" error

### Requirement: Workspace diff against baseline
The Workspace SHALL produce a ChangeSet (added/modified/deleted files with content diffs) relative to the baseline snapshot at creation time.

#### Scenario: Diff after modifications
- **WHEN** Task A's workspace modified src/auth.rs and added src/oauth.rs
- **THEN** `workspace.diff()` returns a ChangeSet with modified: [src/auth.rs], added: [src/oauth.rs]

### Requirement: Workspace merge into target
The Workspace SHALL support merging its changes into a target workspace (e.g., the main project). Merge conflicts SHALL be detected and reported.

#### Scenario: Clean merge
- **WHEN** Task A's workspace merges into main project and no conflicting changes exist
- **THEN** merge result is Clean and all changes are applied

#### Scenario: Merge conflict detected
- **WHEN** Task A's workspace and main project both modified the same lines of src/auth.rs
- **THEN** merge result is Conflict with the conflicting file paths listed

### Requirement: Non-git mirror uses CoW when available
When creating a mirror for a non-git project, the system SHALL attempt Copy-on-Write cloning (APFS clonefile on macOS, btrfs/xfs reflink on Linux) for instant, zero-copy duplication. If CoW is unavailable, it SHALL fall back to full copy.

#### Scenario: CoW on macOS APFS
- **WHEN** mirror is created on macOS with APFS filesystem
- **THEN** clonefile is used and mirror creation is near-instant regardless of project size

#### Scenario: Fallback to full copy
- **WHEN** mirror is created on a filesystem without CoW support
- **THEN** full directory copy is performed with a progress indication

### Requirement: agentignore excludes large directories
The system SHALL respect a `.agentignore` file (similar to .gitignore) to exclude directories like node_modules/, target/, dist/ from the mirror/worktree. Excluded directories SHALL be symlinked from the main project for read access.

#### Scenario: node_modules excluded but symlinked
- **WHEN** a project has node_modules/ listed in .agentignore
- **THEN** node_modules/ is not imported into the mirror
- **AND** the worktree contains a symlink to the main project's node_modules/

### Requirement: Workspace cleanup with configurable delay
After a Task is Archived, its workspace SHALL be scheduled for cleanup after a configurable delay (default 3 days). The Cold Store context is NOT affected by workspace cleanup.

#### Scenario: Delayed cleanup
- **WHEN** Task A is Archived
- **THEN** its worktree is marked for cleanup after the configured delay
- **AND** the worktree remains accessible until the delay expires

#### Scenario: Cold store survives cleanup
- **WHEN** Task A's worktree is cleaned up
- **THEN** Task A's full context in Cold Store remains intact and recallable
