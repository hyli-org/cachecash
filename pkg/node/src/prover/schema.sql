CREATE TABLE rollup_proofs (
    height bigint NOT NULL,
    old_root bytea NOT NULL,
    proof bytea NOT NULL,
    added_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (height)
);
