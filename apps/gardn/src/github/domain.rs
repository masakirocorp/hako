use serde::{Deserialize, Serialize};

use super::{diff::DiffFile, GithubRepository};

pub type GithubResult<T> = Result<T, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    PullRequest,
    Issue,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Queue {
    Authored,
    ReviewRequested,
    Assigned,
    Mentioned,
    All,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemKey {
    pub repository: GithubRepository,
    pub number: u64,
    pub kind: ItemKind,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page<T> {
    pub items: Vec<T>,
    pub next_cursor: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueRequest {
    pub kind: ItemKind,
    pub queue: Queue,
    pub repository: Option<GithubRepository>,
    pub cursor: Option<String>,
    pub page_size: usize,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Viewer {
    pub login: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    pub login: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Label {
    pub name: String,
    pub color: String,
    pub description: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Summary {
    pub key: ItemKey,
    pub title: String,
    pub url: String,
    pub state: String,
    pub author: Option<String>,
    pub updated_at: String,
    pub created_at: String,
    pub is_draft: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryDetails {
    pub repository: GithubRepository,
    pub description: Option<String>,
    pub html_url: String,
    pub private: bool,
    pub archived: bool,
    pub default_branch: Option<String>,
    pub stargazers_count: u64,
    pub forks_count: u64,
    pub open_issues_count: u64,
    pub open_pull_requests_count: u64,
    pub pushed_at: Option<String>,
    pub allow_merge_commit: bool,
    pub allow_squash_merge: bool,
    pub allow_rebase_merge: bool,
    pub allow_auto_merge: Option<bool>,
    pub permissions: Permissions,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Permissions {
    pub admin: bool,
    pub push: bool,
    pub pull: bool,
    pub maintain: Option<bool>,
    pub triage: Option<bool>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Comment {
    pub id: u64,
    pub body: String,
    pub html_url: String,
    pub created_at: String,
    pub updated_at: String,
    pub user: Option<Viewer>,
    pub path: Option<String>,
    pub line: Option<u64>,
    pub original_line: Option<u64>,
    pub side: Option<Side>,
    pub start_line: Option<u64>,
    pub start_side: Option<Side>,
    pub in_reply_to_id: Option<u64>,
    pub commit_id: Option<String>,
    pub original_commit_id: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Review {
    pub id: u64,
    pub user: Option<Viewer>,
    pub body: String,
    pub state: String,
    pub submitted_at: Option<String>,
    pub html_url: String,
    pub commit_id: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub url: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeState {
    pub node_id: String,
    pub head_sha: String,
    pub head_branch: String,
    pub base_branch: String,
    pub mergeable: String,
    pub merge_state_status: String,
    pub review_decision: Option<String>,
    pub auto_merge_enabled: bool,
    pub queue_enabled: bool,
    pub viewer_can_update: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemDetails {
    pub summary: Summary,
    pub body: Option<String>,
    pub labels: Vec<Label>,
    pub assignees: Vec<Viewer>,
    pub comments: Vec<Comment>,
    pub review_comments: Vec<Comment>,
    pub reviews: Vec<Review>,
    pub checks: Vec<Check>,
    pub merge: Option<MergeState>,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub changed_files: Option<u64>,
    pub locked: bool,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Overview {
    pub authored: Page<Summary>,
    pub review_requested: Page<Summary>,
    pub assigned_issues: Page<Summary>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: u64,
    pub run_number: u64,
    pub run_attempt: u64,
    pub name: Option<String>,
    pub display_title: String,
    pub event: String,
    pub head_branch: Option<String>,
    pub head_sha: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub html_url: String,
    pub created_at: String,
    pub updated_at: String,
    pub run_started_at: Option<String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowJob {
    pub id: u64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
    pub html_url: String,
    pub steps: Vec<WorkflowStep>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowStep {
    pub number: u64,
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}
#[derive(Debug, Clone)]
pub struct RunDetails {
    pub run: WorkflowRun,
    pub jobs: Vec<WorkflowJob>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Side {
    Left,
    Right,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineComment {
    pub body: String,
    pub commit_id: String,
    pub path: String,
    pub line: u64,
    pub side: Side,
    pub start: Option<(u64, Side)>,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum CommentKind {
    General,
    Review,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ReviewEvent {
    Comment,
    Approve,
    RequestChanges,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MergeMethod {
    Merge,
    Squash,
    Rebase,
}
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum MergeAction {
    Now(MergeMethod),
    Auto(MergeMethod),
    DisableAuto,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GithubMutation {
    Comment {
        item: ItemKey,
        body: String,
    },
    Reply {
        item: ItemKey,
        comment_id: u64,
        body: String,
    },
    EditComment {
        item: ItemKey,
        kind: CommentKind,
        comment_id: u64,
        body: String,
    },
    DeleteComment {
        item: ItemKey,
        kind: CommentKind,
        comment_id: u64,
    },
    InlineComment {
        item: ItemKey,
        comment: InlineComment,
    },
    Review {
        item: ItemKey,
        head_sha: String,
        event: ReviewEvent,
        body: String,
    },
    Labels {
        item: ItemKey,
        add: Vec<String>,
        remove: Vec<String>,
    },
    Draft {
        item: ItemKey,
        head_sha: String,
        draft: bool,
    },
    Close {
        item: ItemKey,
    },
    Merge {
        item: ItemKey,
        head_sha: String,
        action: MergeAction,
    },
}
impl GithubMutation {
    pub fn item(&self) -> &ItemKey {
        match self {
            Self::Comment { item, .. }
            | Self::Reply { item, .. }
            | Self::EditComment { item, .. }
            | Self::DeleteComment { item, .. }
            | Self::InlineComment { item, .. }
            | Self::Review { item, .. }
            | Self::Labels { item, .. }
            | Self::Draft { item, .. }
            | Self::Close { item }
            | Self::Merge { item, .. } => item,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GithubRequest {
    Viewer,
    Organizations {
        cursor: Option<String>,
        page_size: usize,
    },
    ScopeRepositories {
        cursor: Option<String>,
        page_size: usize,
    },
    Repositories {
        cursor: Option<String>,
        page_size: usize,
    },
    Repository(GithubRepository),
    Queue(QueueRequest),
    Overview {
        repository: Option<GithubRepository>,
        page_size: usize,
    },
    Details(ItemKey),
    Diff {
        item: ItemKey,
        head_sha: String,
    },
    Labels(GithubRepository),
    Runs {
        repository: GithubRepository,
        head_sha: Option<String>,
        cursor: Option<String>,
        page_size: usize,
    },
    Run {
        repository: GithubRepository,
        run_id: u64,
    },
    Mutate(GithubMutation),
}
#[derive(Debug, Clone)]
pub enum GithubResponse {
    Viewer(Viewer),
    Organizations(Page<Organization>),
    ScopeRepositories(Page<GithubRepository>),
    Repositories(Page<GithubRepository>),
    Repository(RepositoryDetails),
    Queue(Page<Summary>),
    Overview(Overview),
    Details(Box<ItemDetails>),
    Diff(Vec<DiffFile>),
    Labels(Vec<Label>),
    Runs(Page<WorkflowRun>),
    Run(RunDetails),
    Mutated,
}
