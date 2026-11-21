pub use school_day_core::lessons::*;

pub fn read_schedule(
    bells: Option<crate::bells::BellSchedule>,
) -> Result<(SchoolData, Option<std::path::PathBuf>), String> {
    let (mut data, backup) = crate::storage::open_schedule()?;
    if data.bells.is_none() {
        data.bells = bells;
    }
    Ok((data, backup))
}
