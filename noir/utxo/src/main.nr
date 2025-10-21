use common::{InputNote, Note};
use utxo_lib::utxo_main;

fn main(
    input_notes: [InputNote; 2],
    output_notes: [Note; 2],
    pmessage4: Field,
    commitments: pub [Field; 4],
    messages: pub [Field; 5],
) {
    utxo_main(
        input_notes,
        output_notes,
        pmessage4,
        commitments,
        messages,
    );
}
