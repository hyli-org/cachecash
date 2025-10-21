#!/usr/bin/env bash

set -euo pipefail

# Compile the program
NARGO=${NARGO:-nargo}
$NARGO compile --workspace

REPO_ROOT=$(git rev-parse --show-toplevel)
BACKEND=${BACKEND:-bb}

# Clean target
rm -r $REPO_ROOT/noir/target

# Compile the program
nargo compile --workspace

# Create the fixtures directory if it doesn't exist
mkdir -p $REPO_ROOT/fixtures/programs

# Copy the compiled programs to the fixtures directory
cp -r $REPO_ROOT/noir/target/* $REPO_ROOT/fixtures/programs/

# Create the keys directory if it doesn't exist
mkdir -p $REPO_ROOT/fixtures/keys

# Get all program names from the workspace - the ordering of these is important,
# as the hash from utxo is used in agg_utxo, and agg_utxo used in agg_agg
PROGRAMS=("utxo" "agg_utxo" "agg_agg" "signature" "points" "migrate")

# Define which programs should use the recursive flag
RECURSIVE_PROGRAMS=("agg_utxo" "utxo")

# Function to check if a program should use recursive flag
is_recursive() {
  local program_name="$1"
  for p in "${RECURSIVE_PROGRAMS[@]}"; do
    if [[ "$p" == "$program_name" ]]; then
      return 0  # True in bash
    fi
  done
  return 1  # False in bash
}

# Generate verification keys for each program
for NAME in "${PROGRAMS[@]}"; do
  oracle_hash_args=()
  if [ "$NAME" == "agg_agg" ]; then
    oracle_hash_args=("--oracle_hash" "keccak")
  fi

  echo "================"
  echo "$(echo "$NAME" | tr '[:lower:]' '[:upper:]')"
  echo "================"

  if is_recursive "$NAME"; then
    echo "Generating verification key for $NAME with recursive flag"
    $BACKEND write_vk ${oracle_hash_args[@]} --scheme ultra_honk --honk_recursion 1 --init_kzg_accumulator -b $REPO_ROOT/fixtures/programs/${NAME}.json -o $REPO_ROOT/fixtures/keys/ --output_format bytes_and_fields \
      && mv $REPO_ROOT/fixtures/keys/{vk,${NAME}_key} && mv $REPO_ROOT/fixtures/keys/{vk_fields.json,${NAME}_key_fields.json}
  else
    echo "Generating verification key for $NAME"
    $BACKEND write_vk ${oracle_hash_args[@]} --scheme ultra_honk -b $REPO_ROOT/fixtures/programs/${NAME}.json -o $REPO_ROOT/fixtures/keys/ --output_format bytes_and_fields \
      && mv $REPO_ROOT/fixtures/keys/{vk,${NAME}_key} && mv $REPO_ROOT/fixtures/keys/{vk_fields.json,${NAME}_key_fields.json}
  fi

  # Print verification key hash as u256 and hex
  echo "Verification key hash for $NAME:"
  VK_HASH_OUTPUT=$(cd $REPO_ROOT && cargo run --bin vk_hash -- $REPO_ROOT/fixtures/keys/${NAME}_key_fields.json)
  echo "$VK_HASH_OUTPUT" | sed 's/^/  /'
  echo ""

  # Update agg_utxo/src/main.nr with the UTXO verification key hash
  if [ "$NAME" == "utxo" ]; then
    UTXO_VK_HASH=$(echo "$VK_HASH_OUTPUT" | grep "u256:" | cut -d' ' -f2)
    echo "Updating agg_utxo/src/main.nr with UTXO verification key hash: $UTXO_VK_HASH"
    sed -i.bak "s/global UTXO_VERIFICATION_KEY_HASH: Field = [0-9]*;/global UTXO_VERIFICATION_KEY_HASH: Field = $UTXO_VK_HASH;/" $REPO_ROOT/noir/agg_utxo/src/main.nr
    rm $REPO_ROOT/noir/agg_utxo/src/main.nr.bak
  fi

  # Update agg_agg/src/main.nr with the agg_utxo verification key hash
  if [ "$NAME" == "agg_utxo" ]; then
    AGG_UTXO_VK_HASH=$(echo "$VK_HASH_OUTPUT" | grep "u256:" | cut -d' ' -f2)
    echo "Updating agg_agg/src/main.nr with agg_utxo verification key hash: $AGG_UTXO_VK_HASH"
    sed -i.bak "s/global AGG_UTXO_VERIFICATION_KEY_HASH: Field = [0-9]*;/global AGG_UTXO_VERIFICATION_KEY_HASH: Field = $AGG_UTXO_VK_HASH;/" $REPO_ROOT/noir/agg_agg/src/main.nr
    rm $REPO_ROOT/noir/agg_agg/src/main.nr.bak
  fi

  $BACKEND write_solidity_verifier --scheme ultra_honk -k $REPO_ROOT/fixtures/keys/${NAME}_key -o $REPO_ROOT/eth/noir/${NAME}.sol
  sed -i.bak 's/external pure/internal pure/g' $REPO_ROOT/eth/noir/${NAME}.sol
  rm $REPO_ROOT/eth/noir/${NAME}.sol.bak
  if [[ "$(uname)" == "Darwin" ]]; then
    SOLC=$REPO_ROOT/fixtures/binaries/solc-v0.8.29-macos
  else
    SOLC=$REPO_ROOT/fixtures/binaries/solc-v0.8.29-linux
  fi
  $SOLC --combined-json bin --revert-strings strip --optimize --optimize-runs 1 $REPO_ROOT/eth/noir/$NAME.sol | jq -r ".contracts[\"$REPO_ROOT/eth/noir/$NAME.sol:HonkVerifier\"].bin" > $REPO_ROOT/eth/contracts/noir/${NAME}_HonkVerifier.bin
done

echo "Successfully copied compiled programs to fixtures/keys/programs"
