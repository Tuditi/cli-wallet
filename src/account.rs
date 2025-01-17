// Copyright 2020 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

use crate::print_error;

use anyhow::Result;
use clap::{App, ArgMatches};
use dialoguer::Input;
use iota_wallet::{
    account::AccountHandle,
    address::Address,
    client::ClientOptionsBuilder,
    message::{Message, MessageId, MessagePayload, MessageType, TransactionEssence, Transfer},
};

use std::{num::NonZeroU64, process::Command, str::FromStr};

fn print_message(message: &Message) {
    println!("MESSAGE {}", message.id());
    if let Some(MessagePayload::Transaction(tx)) = message.payload() {
        let TransactionEssence::Regular(essence) = tx.essence();
        println!("--- Value: {:?}", essence.value());
    }
    println!("--- Timestamp: {:?}", message.timestamp());
    println!(
        "--- Broadcasted: {}, confirmed: {}",
        message.broadcasted(),
        match message.confirmed() {
            Some(c) => c.to_string(),
            None => "unknown".to_string(),
        }
    );
}

async fn print_address(account_handle: &AccountHandle, address: &Address) {
    println!("ADDRESS {:?}", address.address().to_bech32());
    println!("Total balance: {}", address.balance());
    println!(
        "--- Balance: {}",
        account_handle
            .read()
            .await
            .address_available_balance(address)
            .await
            .unwrap()
    );
    println!("--- Index: {}", address.key_index());
    println!("--- Change address: {}", address.internal());
}

// `list-messages` command
async fn list_messages_command(account_handle: &AccountHandle, matches: &ArgMatches) -> Result<()> {
    if let Some(matches) = matches.subcommand_matches("list-messages") {
        if let Some(id) = matches.value_of("id") {
            if let Ok(message_id) = MessageId::from_str(id) {
                let account = account_handle.read().await;
                if let Some(message) = account.get_message(&message_id).await {
                    print_message(&message);
                } else {
                    println!("Message not found");
                }
            } else {
                println!("Message id must be a hex string of length 64");
            }
        } else {
            let account = account_handle.read().await;
            let message_type = if let Some(message_type) = matches.value_of("type") {
                match message_type {
                    "received" => Some(MessageType::Received),
                    "sent" => Some(MessageType::Sent),
                    "failed" => Some(MessageType::Failed),
                    "unconfirmed" => Some(MessageType::Unconfirmed),
                    "value" => Some(MessageType::Value),
                    _ => panic!("unexpected message type"),
                }
            } else {
                None
            };
            let messages = account.list_messages(0, 0, message_type).await?;
            if messages.is_empty() {
                println!("No messages found");
            } else {
                messages.iter().for_each(|m| print_message(m));
            }
        }
    }
    Ok(())
}

// `list-addresses` command
async fn list_addresses_command(account_handle: &AccountHandle, matches: &ArgMatches) {
    if matches.subcommand_matches("list-addresses").is_some() {
        let account = account_handle.read().await;
        let addresses = account.addresses();
        if addresses.is_empty() {
            println!("No addresses found");
        } else {
            for address in addresses {
                print_address(account_handle, address).await;
            }
        }
    }
}

// `sync` command
async fn sync_account_command(account_handle: &AccountHandle, matches: &ArgMatches) -> Result<()> {
    if let Some(matches) = matches.subcommand_matches("sync") {
        let mut sync = account_handle.sync().await;
        if let Some(gap_limit) = matches.value_of("gap") {
            if let Ok(limit) = gap_limit.parse::<usize>() {
                println!("Syncing with gap limit {}", limit);
                sync = sync.gap_limit(limit);
            } else {
                return Err(anyhow::anyhow!("Gap limit must be a number"));
            }
        }
        let synced = sync.execute().await?;
        for address in synced.addresses() {
            print_address(account_handle, address).await;
        }
        for message in synced.messages() {
            print_message(message);
        }
    }
    Ok(())
}

// `address` command
async fn generate_address_command(account_handle: &AccountHandle, matches: &ArgMatches) -> Result<()> {
    if matches.subcommand_matches("address").is_some() {
        let address = account_handle.generate_address().await?;
        print_address(account_handle, &address).await;
    }
    Ok(())
}

// `balance` command
async fn balance_command(account_handle: &AccountHandle, matches: &ArgMatches) -> Result<()> {
    if matches.subcommand_matches("balance").is_some() {
        let account = account_handle.read().await;
        println!("{:?}", account.balance().await?);
    }
    Ok(())
}

// `transfer` command
async fn transfer_command(account_handle: &AccountHandle, matches: &ArgMatches) -> Result<()> {
    if let Some(matches) = matches.subcommand_matches("transfer") {
        let address = matches.value_of("address").unwrap().to_string();
        let amount = matches.value_of("amount").unwrap();
        if let Ok(address) = iota_wallet::address::parse(address) {
            if let Ok(amount) = amount.parse::<u64>() {
                let transfer = Transfer::builder(
                    address,
                    NonZeroU64::new(amount).ok_or_else(|| anyhow::anyhow!("amount can't be zero"))?,
                    None,
                )
                .finish();

                let message = account_handle.transfer(transfer).await?;
                print_message(&message);
            } else {
                return Err(anyhow::anyhow!("Amount must be a number"));
            }
        } else {
            return Err(anyhow::anyhow!("Address must be a bech32 string"));
        }
    }
    Ok(())
}

enum ReplayAction {
    Promote,
    Retry,
    Reattach,
}

// promotes, retries or reattaches a message
async fn replay_message(account_handle: &AccountHandle, action: ReplayAction, message_id: &str) -> Result<()> {
    if let Ok(message_id) = MessageId::from_str(message_id) {
        let message = match action {
            ReplayAction::Promote => account_handle.promote(&message_id).await?,
            ReplayAction::Retry => account_handle.retry(&message_id).await?,
            ReplayAction::Reattach => account_handle.reattach(&message_id).await?,
        };
        print_message(&message);
    } else {
        println!("Message id must be a hex string of length 64");
    }
    Ok(())
}

// `promote` command
async fn promote_message_command(account_handle: &AccountHandle, matches: &ArgMatches) -> Result<()> {
    if let Some(matches) = matches.subcommand_matches("promote") {
        let message_id = matches.value_of("id").unwrap();
        replay_message(account_handle, ReplayAction::Promote, message_id).await?;
    }
    Ok(())
}

// `retry` command
async fn retry_message_command(account_handle: &AccountHandle, matches: &ArgMatches) -> Result<()> {
    if let Some(matches) = matches.subcommand_matches("retry") {
        let message_id = matches.value_of("id").unwrap();
        replay_message(account_handle, ReplayAction::Retry, message_id).await?;
    }
    Ok(())
}

// `reattach` command
async fn reattach_message_command(account_handle: &AccountHandle, matches: &ArgMatches) -> Result<()> {
    if let Some(matches) = matches.subcommand_matches("reattach") {
        let message_id = matches.value_of("id").unwrap();
        replay_message(account_handle, ReplayAction::Reattach, message_id).await?;
    }
    Ok(())
}

// `set-node` command
async fn set_node_command(account_handle: &AccountHandle, matches: &ArgMatches) -> Result<()> {
    if let Some(matches) = matches.subcommand_matches("set-node") {
        let node = matches.value_of("node").unwrap();
        account_handle
            .set_client_options(ClientOptionsBuilder::new().with_nodes(&[node])?.build()?)
            .await?;
    }
    Ok(())
}

// `set-alias` command
async fn set_alias_command(account_handle: &AccountHandle, matches: &ArgMatches) -> Result<()> {
    if let Some(matches) = matches.subcommand_matches("set-alias") {
        let alias = matches.value_of("alias").unwrap();
        account_handle.set_alias(alias).await?;
    }
    Ok(())
}

// account prompt commands
async fn account_commands(account_handle: &AccountHandle, matches: &ArgMatches) -> Result<()> {
    list_messages_command(account_handle, matches).await?;
    list_addresses_command(account_handle, matches).await;
    sync_account_command(account_handle, matches).await?;
    generate_address_command(account_handle, matches).await?;
    balance_command(account_handle, matches).await?;
    transfer_command(account_handle, matches).await?;
    promote_message_command(account_handle, matches).await?;
    retry_message_command(account_handle, matches).await?;
    reattach_message_command(account_handle, matches).await?;
    set_node_command(account_handle, matches).await?;
    set_alias_command(account_handle, matches).await?;
    Ok(())
}

// loop on the account prompt
pub async fn account_prompt(account_cli: &App<'_>, account_handle: AccountHandle) {
    loop {
        let exit = account_prompt_internal(account_cli, account_handle.clone()).await;
        if exit {
            break;
        }
    }
}

// loop on the account prompt
pub async fn account_prompt_internal(account_cli: &App<'_>, account_handle: AccountHandle) -> bool {
    let alias = account_handle.alias().await;
    let command: String = Input::new()
        .with_prompt(format!("Account `{}` command (h for help)", alias))
        .interact_text()
        .unwrap();

    match command.as_str() {
        "h" => {
            let mut cli = account_cli.clone();
            cli.print_help().unwrap();
        }
        "clear" => {
            let _ = Command::new("clear").status();
        }
        _ => {
            match account_cli
                .clone()
                .try_get_matches_from(command.split(' ').collect::<Vec<&str>>())
            {
                Ok(matches) => {
                    if matches.subcommand_matches("exit").is_some() {
                        return true;
                    }

                    if let Err(e) = account_commands(&account_handle, &matches).await {
                        print_error(e);
                    }
                }
                Err(e) => {
                    println!("{}", e.to_string());
                }
            }
        }
    }

    false
}
