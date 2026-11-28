//! The label of the build: the PR and the commit that CI built.

/// The label of this build, from the variables that CI gives the build.
pub fn label() -> String {
    label_of(option_env!("ISOTHERM_PR"), option_env!("ISOTHERM_SHA"))
}

/// The label of a build with this PR number and this commit. An empty value
/// is the same as no value.
pub fn label_of(pr: Option<&str>, sha: Option<&str>) -> String {
    let pr = pr.map(str::trim).filter(|s| !s.is_empty());
    let sha = sha.map(str::trim).filter(|s| !s.is_empty());
    let short = sha.map(|s| &s[..s.len().min(7)]);
    match (pr, short) {
        (Some(pr), Some(sha)) => format!("PR {pr} · commit {sha}"),
        (Some(pr), None) => format!("PR {pr}"),
        (None, Some(sha)) => format!("Commit {sha}"),
        (None, None) => "Local build".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ci_build_shows_its_pr_and_its_short_commit() {
        let sha = Some("abc1234def5678");
        assert_eq!(label_of(Some("693"), sha), "PR 693 · commit abc1234");
        assert_eq!(label_of(None, sha), "Commit abc1234");
        assert_eq!(label_of(Some("693"), None), "PR 693");
    }

    #[test]
    fn a_build_with_no_or_empty_values_is_a_local_build() {
        assert_eq!(label_of(None, None), "Local build");
        assert_eq!(label_of(Some(""), Some("")), "Local build");
    }
}
