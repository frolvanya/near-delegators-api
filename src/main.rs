mod extensions;
mod methods;

#[macro_use]
extern crate rocket;

use std::io::{Read, Write};

use git2::{Cred, ObjectType, PushOptions, RemoteCallbacks, Repository, Signature};

use near_jsonrpc_client::JsonRpcClient;
use rocket::http::Status;

const STAKE_DELEGATORS_FILENAME: &str = "stake_delegators.json";

fn push_updates() -> color_eyre::Result<()> {
    let repo_path = std::env::current_dir()?;

    let repo = Repository::open(&repo_path)?;

    let mut index = repo.index().unwrap();
    let add_result = index.add_all(&["."], git2::IndexAddOption::DEFAULT, None);

    if let Err(e) = add_result {
        eprintln!("Failed to add all files to the index: {}", e);
        return Err(e.into());
    }
    index.write().unwrap();

    let head = repo.head()?;
    let commit = repo.find_commit(head.peel(ObjectType::Commit)?.id())?;
    let tree = commit.tree()?;
    let parent_commit = vec![&commit];

    let signature = Signature::now("Ivan Frolov", "frolvanya@gmail.com")?;

    let new_commit_id = repo.commit(
        Some("HEAD"),
        &signature,
        &signature,
        format!("chore: updated {STAKE_DELEGATORS_FILENAME}").as_str(),
        &tree,
        &parent_commit,
    )?;

    println!(
        "Changes committed successfully! New commit ID: {}",
        new_commit_id
    );

    let remote_name = "origin";
    let branch_name = "master";
    let mut remote = repo.find_remote(remote_name)?;
    println!("Connected to remote: {}", remote.name().unwrap());

    let mut callbacks = RemoteCallbacks::new();
    callbacks.credentials(|_url, username_from_url, _allowed_types| {
        Cred::ssh_key(
            username_from_url.unwrap(),
            None,
            std::path::Path::new(&format!("{}/.ssh/id_rsa", std::env::var("HOME").unwrap())),
            None,
        )
    });
    let mut po = PushOptions::new();
    po.remote_callbacks(callbacks);
    // remote.connect_auth(git2::Direction::Push, Some(callbacks), None)?;
    // println!("Connected to remote: {}", remote.name().unwrap());

    let refspec = format!("refs/heads/{}:refs/heads/{}", branch_name, branch_name);

    remote.push(&[refspec.as_str()], Some(&mut po))?;

    println!("Changes pushed successfully!");

    Ok(())
}

#[post("/")]
async fn webhook() -> Status {
    let json_rpc_client = JsonRpcClient::connect("https://rpc.mainnet.near.org");

    if let Ok(delegators) = methods::get_all_delegators(&json_rpc_client).await {
        // println!("{:?}", delegators);
        let file_result = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(STAKE_DELEGATORS_FILENAME);

        let Ok(mut file) = file_result else { return Status::NotFound };

        match serde_json::to_string_pretty(&delegators) {
            Ok(json) => {
                if file.write_all(json.as_bytes()).is_err() {
                    return Status::NotFound;
                }
            }
            Err(_) => {
                return Status::NotFound;
            }
        }

        push_updates().unwrap();
    }

    Status::Ok
}

#[launch]
fn rocket() -> _ {
    rocket::build().mount("/", routes![webhook])
}
