pub use crate::ordeal::{
    ExpectedSpec, OrdealConfig as GauntletConfig, OrdealOutput as GauntletOutput, OutputSpec,
    TaskSpec, TaskSuite, LEGACY_GAUNTLET_TASKS_PATH as GAUNTLET_TASKS_PATH,
};

pub use crate::ordeal::{
    run_ordeal as run_gauntlet, StepSpec, LEGACY_GAUNTLET_TASKS_PATH,
    ORDEAL_TASK_COUNT as GAUNTLET_TASK_COUNT,
};
