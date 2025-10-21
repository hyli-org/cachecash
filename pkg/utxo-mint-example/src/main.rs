use barretenberg::Prove;
use element::Element;
use zk_primitives::{
    InputNote, Note, ToBytes, Utxo, UtxoKind, bridged_polygon_usdc_note_kind,
    get_address_for_private_key,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    color_eyre::install()?;

    let bridged_note_kind = bridged_polygon_usdc_note_kind();

    let secret_key = Element::new(101);
    let address = get_address_for_private_key(secret_key);

    let input_a = Note::new_with_psi(address, Element::new(15), Element::new(1));
    let input_b = Note::new_with_psi(address, Element::new(5), Element::new(2));

    let minted_note = Note {
        kind: Element::new(2),
        contract: bridged_note_kind,
        address: Element::ZERO,
        psi: Element::ZERO,
        value: Element::new(1),
    };
    let recipient_note = Note::new_with_psi(address, Element::new(29), Element::new(3));

    let utxo_mint = Utxo::new(
        UtxoKind::Mint,
        [
            InputNote::new(input_a.clone(), secret_key),
            InputNote::new(input_b.clone(), secret_key),
        ],
        [minted_note, recipient_note],
        None,
    );

    println!("Input total  : {}", utxo_mint.input_value());
    println!("Output total : {}", utxo_mint.output_value());
    println!(
        "Minted value : {}",
        utxo_mint.output_value() - utxo_mint.input_value()
    );
    println!("Mint hash    : {}", utxo_mint.mint_hash());

    let proof = utxo_mint.prove()?;

    println!("Proof size   : {} bytes", proof.to_bytes().len());
    println!(
        "Public msgs  : {:?}",
        utxo_mint
            .messages()
            .iter()
            .map(|msg| msg.to_string())
            .collect::<Vec<_>>()
    );
    println!("Mint proof generated successfully!");

    Ok(())
}
