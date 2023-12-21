use color_eyre::Result;
use git2::{Cred, Signature};

pub const STAKE_DELEGATORS_FILENAME: &str = "stake_delegators.json";

fn find_last_commit(repo: &git2::Repository) -> Result<git2::Commit, git2::Error> {
    let obj = repo.head()?.resolve()?.peel(git2::ObjectType::Commit)?;
    obj.into_commit()
        .map_err(|_| git2::Error::from_str("Couldn't find commit"))
}

pub fn push() -> Result<()> {
    info!("Opening git repo");
    let repo = git2::Repository::open(std::path::Path::new("."))?;

    info!("Adding `{}` file", STAKE_DELEGATORS_FILENAME);
    let mut index = repo.index()?;

    index.add_path(std::path::Path::new(STAKE_DELEGATORS_FILENAME))?;
    index.write()?;

    let oid = index.write_tree()?;
    let parent_commit = find_last_commit(&repo)?;
    let tree = repo.find_tree(oid)?;

    let signature = Signature::now("Ivan Frolov", "frolvanya@gmail.com")?;

    info!("Committing changes");
    repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        "chore: updated stake delegators",
        &tree,
        &[&parent_commit],
    )?;

    let branch_name = "master";
    let mut remote = repo.find_remote("origin")?;

    let mut callbacks = git2::RemoteCallbacks::new();
    let mut options = git2::PushOptions::new();

    callbacks.credentials(|_url, username_from_url, _allowed_types| {
        let Some(username_from_url) = username_from_url else { return Err(git2::Error::from_str("Couldn't find username")) };
        let Ok(home) = std::env::var("HOME") else { return Err(git2::Error::from_str("Couldn't find home")) };
        Cred::ssh_key(
            username_from_url,
            None,
            std::path::Path::new(&format!(
                "{}/.ssh/id_rsa",
                home
            )),
            None,
        )
    });
    options.remote_callbacks(callbacks);

    info!("Pushing to git");
    remote.push(
        &[format!("refs/heads/{branch_name}:refs/heads/{branch_name}")],
        Some(&mut options),
    )?;

    Ok(())
}
