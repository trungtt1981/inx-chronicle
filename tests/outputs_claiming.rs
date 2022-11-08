// Copyright 2022 IOTA Stiftung
// SPDX-License-Identifier: Apache-2.0

mod common;

#[cfg(feature = "rand")]
mod test_rand {
    use chronicle::{
        db::collections::OutputCollection,
        types::{
            ledger::{LedgerOutput, LedgerSpent, MilestoneIndexTimestamp, RentStructureBytes, SpentMetadata},
            stardust::block::{
                output::{BasicOutput, OutputAmount, OutputId},
                payload::TransactionId,
                BlockId, Output,
            },
        },
    };
    use decimal::d128;

    use super::common::{setup_collection, setup_database, teardown};

    fn rand_output_with_value(amount: OutputAmount) -> Output {
        // We use `BasicOutput`s in the genesis.
        let mut output = BasicOutput::rand(&iota_types::block::protocol::protocol_parameters());
        output.amount = amount;
        Output::Basic(output)
    }

    #[tokio::test]
    async fn test_claiming() {
        let db = setup_database("test-claiming").await.unwrap();
        let output_collection = setup_collection::<OutputCollection>(&db).await.unwrap();

        let unspent_outputs = (1..=5)
            .map(|i| LedgerOutput {
                output_id: OutputId::rand(),
                rent_structure: RentStructureBytes {
                    num_key_bytes: 0,
                    num_data_bytes: 100,
                },
                output: rand_output_with_value(i.into()),
                block_id: BlockId::rand(),
                booked: MilestoneIndexTimestamp {
                    milestone_index: 0.into(),
                    milestone_timestamp: 0.into(),
                },
            })
            .collect::<Vec<_>>();

        output_collection
            .insert_unspent_outputs(&unspent_outputs)
            .await
            .unwrap();

        let spent_outputs = unspent_outputs
            .into_iter()
            .take(4) // we spent only the first 4 outputs
            .map(|output| {
                let i = output.output.amount().0;
                LedgerSpent {
                    output,
                    spent_metadata: SpentMetadata {
                        transaction_id: TransactionId::rand(),
                        spent: MilestoneIndexTimestamp {
                            milestone_index: (i as u32).into(),
                            milestone_timestamp: (i as u32 + 10000).into(),
                        },
                    },
                }
            })
            .collect::<Vec<_>>();

        output_collection.update_spent_outputs(&spent_outputs).await.unwrap();

        let claimed = output_collection.get_claimed_token_analytics(1.into()).await.unwrap();
        assert_eq!(claimed.claimed_count, 1);
        assert_eq!(claimed.claimed_value, d128::from(1));

        let claimed = output_collection.get_claimed_token_analytics(2.into()).await.unwrap();
        assert_eq!(claimed.claimed_count, 2);
        assert_eq!(claimed.claimed_value, d128::from((1..=2).sum::<u32>()));

        let claimed = output_collection.get_claimed_token_analytics(3.into()).await.unwrap();
        assert_eq!(claimed.claimed_count, 3);
        assert_eq!(claimed.claimed_value, d128::from((1..=3).sum::<u32>()));

        let claimed = output_collection.get_claimed_token_analytics(4.into()).await.unwrap();
        assert_eq!(claimed.claimed_count, 4);
        assert_eq!(claimed.claimed_value, d128::from((1..=4).sum::<u32>()));

        let claimed = output_collection.get_claimed_token_analytics(5.into()).await.unwrap();
        assert_eq!(claimed.claimed_count, 4);
        assert_eq!(claimed.claimed_value, d128::from((1..=4).sum::<u32>()));

        teardown(db).await;
    }
}