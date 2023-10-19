#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(non_snake_case)]

mod constants;
mod instruction;
mod pb;
mod utils;

use instruction::{parse_instruction, Instruction};
use pb::sf::solana::spl::v1::{Accounts, Arg, Output, SplTokenMeta};
use substreams::log;
use substreams::store::{StoreGet, StoreGetArray};
use substreams_solana::pb::sf::solana::r#type::v1::Block;
use utils::convert_to_date;

#[derive(Default)]
pub struct OuterArg {
    pub instruction_type: String,
    pub input_accounts: Accounts,
    pub arg: Arg,
}

#[substreams::handlers::map]
fn map_block(
    block: Block,
    address_lookup_table_store: StoreGetArray<String>,
) -> Result<Output, substreams::errors::Error> {
    let slot = block.slot;
    let parent_slot = block.parent_slot;
    let timestamp = block.block_time.as_ref().unwrap().timestamp;
    let mut data: Vec<SplTokenMeta> = vec![];

    for trx in block.transactions_owned() {
        if let Some(transaction) = trx.transaction {
            let meta = trx.meta.unwrap();
            let msg = transaction.message.unwrap();
            let mut accounts = vec![];
            let mut writable_accounts = vec![];
            let mut readable_accounts = vec![];

            msg.account_keys
                .into_iter()
                .for_each(|addr| accounts.push(bs58::encode(addr).into_string()));
            msg.address_table_lookups.into_iter().for_each(|addr| {
                let acc = bs58::encode(&addr.account_key).into_string();
                match address_lookup_table_store.get_last(format!("table:{}", acc)) {
                    None => {}
                    Some(accs) => {
                        addr.writable_indexes.into_iter().for_each(|idx| {
                            writable_accounts.push(accs[idx as usize].clone());
                        });
                        addr.readonly_indexes.into_iter().for_each(|idx| {
                            readable_accounts.push(accs[idx as usize].clone());
                        })
                    }
                }
            });

            accounts.append(&mut writable_accounts);
            accounts.append(&mut readable_accounts);

            for (idx, inst) in msg.instructions.into_iter().enumerate() {
                let program = &accounts[inst.program_id_index as usize];

                if program != constants::PROGRAM_ADDRESS {
                    continue;
                }

                let outer_arg = get_outer_arg(inst.data, &inst.accounts, &accounts);

                data.push(SplTokenMeta {
                    block_date: convert_to_date(timestamp),
                    block_time: timestamp,
                    tx_id: bs58::encode(&transaction.signatures[0]).into_string(),
                    dapp: constants::PROGRAM_ADDRESS.to_string(),
                    block_slot: parent_slot,
                    instruction_index: inst.program_id_index,
                    is_inner_instruction: false,
                    inner_instruction_index: 0,
                    instruction_type: outer_arg.instruction_type,
                    input_accounts: outer_arg.input_accounts,
                    args: outer_arg.arg,
                });

                meta.inner_instructions
                    .iter()
                    .filter(|inner_instruction| inner_instruction.index == idx as u32)
                    .for_each(|inner_instruction| {
                        inner_instruction
                            .instructions
                            .iter()
                            .for_each(|inner_inst| {
                                let program = &accounts[inner_inst.program_id_index as usize];
                                if program == constants::PROGRAM_ADDRESS {
                                    let outer_arg = get_outer_arg(
                                        inner_inst.data.clone(),
                                        &inner_inst.accounts,
                                        &accounts,
                                    );

                                    data.push(SplTokenMeta {
                                        block_date: convert_to_date(timestamp),
                                        block_time: timestamp,
                                        tx_id: bs58::encode(&transaction.signatures[0])
                                            .into_string(),
                                        dapp: constants::PROGRAM_ADDRESS.to_string(),
                                        block_slot: parent_slot + 1,
                                        instruction_index: inst.program_id_index,
                                        is_inner_instruction: true,
                                        inner_instruction_index: inner_inst.program_id_index,
                                        instruction_type: outer_arg.instruction_type,
                                        input_accounts: outer_arg.input_accounts,
                                        args: outer_arg.arg,
                                    });
                                }
                            })
                    });
            }
        }
    }

    log::info!("{:#?}", slot);
    return Ok(Output { data });
}

fn get_outer_arg(
    instruction_data: Vec<u8>,
    account_indices: &Vec<u8>,
    accounts: &Vec<String>,
) -> OuterArg {
    let account_args = prepare_account_args(account_indices, accounts);
    let mut outerArg: OuterArg = OuterArg::default();
    let mut arg: Arg = Arg::default();
    let instruction: Instruction = parse_instruction(instruction_data, account_args);

    outerArg.input_accounts = Accounts {
        mint: Some(instruction.instruction_accounts.mint),
        rent_sysvar: Some(instruction.instruction_accounts.rent_sysvar),
        account: Some(instruction.instruction_accounts.account),
        owner: Some(instruction.instruction_accounts.owner),
        signer_accounts: instruction.instruction_accounts.signer_accounts,
        source: Some(instruction.instruction_accounts.source),
        destination: Some(instruction.instruction_accounts.destination),
        delegate: Some(instruction.instruction_accounts.delegate),
        authority: Some(instruction.instruction_accounts.authority),
        payer: Some(instruction.instruction_accounts.payer),
        fund_relocation_sys_program: Some(
            instruction.instruction_accounts.fund_relocation_sys_program,
        ),
        funding_account: Some(instruction.instruction_accounts.funding_account),
        mint_funding_sys_program: Some(instruction.instruction_accounts.mint_funding_sys_program),
    };

    match instruction.name.as_str() {
        "InitializeMint" => {
            outerArg.instruction_type = String::from("InitializeMint");
            arg.decimals = Some(i32::from(instruction.initializeMintArgs.decimals));
            arg.mint_authority =
                get_b58_string(instruction.initializeMintArgs.mint_authority.value);
            arg.freeze_authority_option = Some(i32::from(
                instruction.initializeMintArgs.freeze_authority_option,
            ));
            arg.freeze_authority = get_b58_string(
                instruction
                    .initializeMintArgs
                    .freeze_authority
                    .unwrap_or_default()
                    .value,
            );
        }
        "InitializeAccount" => {
            outerArg.instruction_type = String::from("InitializeAccount");
        }
        "InitializeMultisig" => {
            outerArg.instruction_type = String::from("InitializeMultisig");
            arg.status = Some(i32::from(instruction.initializeMultisigArgs.status));
        }
        "Transfer" => {
            outerArg.instruction_type = String::from("Transfer");
            arg.amount = Some(instruction.transferArgs.amount);
        }
        "Approve" => {
            outerArg.instruction_type = String::from("Approve");
            arg.amount = Some(instruction.approveArgs.amount);
        }
        "Revoke" => {
            outerArg.instruction_type = String::from("Revoke");
        }
        "SetAuthority" => {
            outerArg.instruction_type = String::from("SetAuthority");
            arg.authority_type = Some(instruction.setAuthorityArgs.authority_type.to_string());
            arg.new_authority_option =
                Some(i32::from(instruction.setAuthorityArgs.new_authority_option));
            if instruction.setAuthorityArgs.new_authority.is_some() {
                arg.new_authority =
                    get_b58_string(instruction.setAuthorityArgs.new_authority.unwrap().value);
            }
        }
        "MintTo" => {
            outerArg.instruction_type = String::from("MintTo");
            arg.amount = Some(instruction.mintToArgs.amount);
        }
        "Burn" => {
            outerArg.instruction_type = String::from("Burn");
            arg.amount = Some(instruction.burnArgs.amount);
        }
        "CloseAccount" => {
            outerArg.instruction_type = String::from("CloseAccount");
        }
        "FreezeAccount" => {
            outerArg.instruction_type = String::from("FreezeAccount");
        }
        "ThawAccount" => {
            outerArg.instruction_type = String::from("ThawAccount");
        }
        "TransferChecked" => {
            outerArg.instruction_type = String::from("TransferChecked");
            arg.amount = Some(instruction.transferCheckedArgs.amount);
            arg.decimals = Some(i32::from(instruction.transferCheckedArgs.decimals));
        }
        "ApproveChecked" => {
            outerArg.instruction_type = String::from("ApproveChecked");
            arg.amount = Some(instruction.approveCheckedArgs.amount);
            arg.decimals = Some(i32::from(instruction.approveCheckedArgs.decimals));
        }
        "MintToChecked" => {
            outerArg.instruction_type = String::from("MintToChecked");
            arg.amount = Some(instruction.mintToCheckedArgs.amount);
            arg.decimals = Some(i32::from(instruction.mintToCheckedArgs.decimals));
        }
        "BurnChecked" => {
            outerArg.instruction_type = String::from("BurnChecked");
            arg.amount = Some(instruction.burnCheckedArgs.amount);
            arg.decimals = Some(i32::from(instruction.burnCheckedArgs.decimals));
        }
        "InitializeAccount2" => {
            outerArg.instruction_type = String::from("InitializeAccount2");
            arg.owner = get_b58_string(instruction.initializeAccount2Args.owner.value);
        }
        "SyncNative" => {
            outerArg.instruction_type = String::from("SyncNative");
        }
        "InitializeAccount3" => {
            outerArg.instruction_type = String::from("InitializeAccount3");
            arg.owner = get_b58_string(instruction.initializeAccount3Args.owner.value);
        }
        "InitializeMultisig2" => {
            outerArg.instruction_type = String::from("InitializeMultisig2");
            arg.status = Some(i32::from(instruction.initializeMultisig2Args.status));
        }
        "InitializeMint2" => {
            outerArg.instruction_type = String::from("InitializeMint2");
            arg.decimals = Some(i32::from(instruction.initializeMint2Args.decimals));
            arg.mint_authority =
                get_b58_string(instruction.initializeMint2Args.mint_authority.value);
            if instruction.initializeMint2Args.freeze_authority.is_some() {
                arg.freeze_authority = get_b58_string(
                    instruction
                        .initializeMint2Args
                        .freeze_authority
                        .unwrap()
                        .value,
                );
            }
        }
        "GetAccountDataSize" => {
            outerArg.instruction_type = String::from("GetAccountDataSize");
            arg.extension_type = Some(i32::from(instruction.getAccountDataSizeArgs.extension_type));
        }
        "InitializeImmutableOwner" => {
            outerArg.instruction_type = String::from("InitializeImmutableOwner");
        }
        "AmountToUiAmount" => {
            outerArg.instruction_type = String::from("AmountToUiAmount");
            arg.amount = Some(instruction.amountToUiAmountArgs.amount);
        }
        "UiAmountToAmount" => {
            outerArg.instruction_type = String::from("UiAmountToAmount");
            arg.ui_amount = Some(instruction.uiAmountToAmountArgs.ui_amount);
        }
        "InitializeMintCloseAuthority" => {
            outerArg.instruction_type = String::from("InitializeMintCloseAuthority");
            arg.owner = get_b58_string(instruction.initializeMintCloseAuthorityArgs.owner.value);
        }
        "TransferFeeExtension" => {
            outerArg.instruction_type = String::from("TransferFeeExtension");
        }
        "ConfidentialTransferExtension" => {
            outerArg.instruction_type = String::from("ConfidentialTransferExtension");
        }
        "DefaultAccountStateExtension" => {
            outerArg.instruction_type = String::from("DefaultAccountStateExtension");
        }
        "Reallocate" => {
            outerArg.instruction_type = String::from("Reallocate");
        }
        "MemoTransferExtension" => {
            outerArg.instruction_type = String::from("MemoTransferExtension");
        }
        "CreateNativeMint" => {
            outerArg.instruction_type = String::from("CreateNativeMint");
        }
        "InitializeNonTransferableMint" => {
            outerArg.instruction_type = String::from("InitializeNonTransferableMint");
        }
        "InterestBearingMintExtension" => {
            outerArg.instruction_type = String::from("InterestBearingMintExtension");
        }
        _ => {
            outerArg.instruction_type = String::from("Unknown Instruction");
        }
    }

    outerArg.arg = arg;

    return outerArg;
}

fn get_b58_string(data: [u8; 32]) -> Option<String> {
    return Some(bs58::encode(data).into_string());
}

fn prepare_account_args(account_indices: &Vec<u8>, accounts: &Vec<String>) -> Vec<String> {
    let mut instruction_accounts: Vec<String> = vec![];
    for (index, &el) in account_indices.iter().enumerate() {
        instruction_accounts.push(accounts.as_slice()[el as usize].to_string());
    }
    return instruction_accounts;
}
