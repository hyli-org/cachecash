# User-to-User Transfer Setup

## Quick Start

### 1. Start the Backend Server

```bash
# From project root
cd /Users/matteo/projects/cachecash
cargo run --package server
```

The server will start on **http://localhost:9002** with these endpoints:
- `POST /api/faucet` - Get funds from faucet
- `POST /api/transfer` - Send funds to another user
- `POST /api/notes` - Upload encrypted notes
- `GET /api/notes/{recipient_tag}` - Fetch encrypted notes
- `DELETE /api/notes/{recipient_tag}/{note_id}` - Delete notes

### 2. Start the Frontend

```bash
# In another terminal
cd front
npm run dev
```

The frontend will connect to **http://localhost:9002** (configured in `front/.env`)

## Using Transfers

### Transfer by Username (Recommended)

1. Click the **"SEND"** button in the game UI
2. Enter recipient's **username** (e.g., "alice")
   - The system will automatically derive their public key
   - You'll see a preview: `→ 1a2b3c4d...9e8f7g6h`
3. Enter amount (must be ≤ your available balance)
4. Click "Send Transfer"

### Transfer by Public Key

You can also send directly to a public key:
```
Recipient: 0x32bf2d2c5796...
```

### Receiving Transfers

No action needed! The app automatically:
- Polls for encrypted notes every 30 seconds
- Decrypts notes with your private key
- Adds funds to your balance
- Shows notification in console

## How It Works

### Client-Side (Privacy Preserved)
1. Selects optimal notes to spend (greedy algorithm)
2. Computes **nullifiers** locally (keeps secret keys private!)
3. Builds transaction with commitments
4. Submits to `/api/transfer`
5. Uploads encrypted note for recipient

### Server-Side
1. Validates inputs (pubkey, amount, commitments)
2. Builds blob transaction
3. Submits to blockchain
4. Returns tx hash + change note

### Recipient
1. Encrypted notes polling hook fetches notes
2. Decrypts with private key (ECDH + AES)
3. Adds to localStorage
4. Balance updates automatically

## Configuration

### Backend (server/src/conf_defaults.toml)
```toml
rest_server_port = 9002
default_faucet_amount = 10
node_url = "http://127.0.0.1:4321"
utxo_contract_name = "hyli_utxo"
```

### Frontend (front/.env)
```
VITE_SERVER_BASE_URL=http://localhost:9002
```

## Troubleshooting

### "404 /api/transfer not found"
**Solution:** Backend server isn't running. Start it with:
```bash
cargo run --package server
```

### "0 Available" in Transfer Modal
**Causes:**
1. Notes still marked as "optimistic" (wait for blockchain confirmation)
2. Notes are pending from another transfer
3. No confirmed notes yet

**Solution:**
- Slice pumpkins to get funds from faucet
- Wait a few seconds for blockchain confirmation
- Check Debug panel (add `?debug=true` to URL)

### Transfer Fails
Check console logs for:
- Insufficient balance errors
- Invalid recipient format
- Network connection issues

## Security Features

✅ **Client-Side Privacy**
- Secret keys never leave browser
- Nullifiers computed locally
- Encrypted note delivery (ECDH + AES)

✅ **Double-Spend Prevention**
- Pending transfer tracking
- Nullifier checking on blockchain
- 5-minute timeout for abandoned transfers

✅ **Input Validation**
- 32-byte public key validation
- Positive amount checking
- Balance verification
- Self-transfer prevention

## Architecture

```
User A (Browser)                Backend Server              Blockchain
     |                               |                            |
     | 1. Select notes               |                            |
     | 2. Compute nullifiers         |                            |
     |    (secret keys stay local!)  |                            |
     |                               |                            |
     | 3. POST /api/transfer ------->|                            |
     |    - commitments              | 4. Build blob tx           |
     |    - nullifiers               | 5. Submit tx ------------->|
     |    - output notes             |                            |
     |                               |<------ 6. Tx hash ---------|
     |<------ 7. Response -----------|                            |
     |    (tx hash, change note)     |                            |
     |                               |                            |
     | 8. Upload encrypted note ---->|                            |
     |    (for recipient)            | 9. Store note              |
     |                               |                            |

User B (Browser)
     |
     | 10. Poll /api/notes (30s)
     |<------ 11. Encrypted notes ---|
     | 12. Decrypt locally
     | 13. Add to balance
```

## Testing Checklist

- [ ] Backend starts without errors
- [ ] Frontend connects to backend
- [ ] Faucet works (slice pumpkins)
- [ ] Transfer modal shows correct balance
- [ ] Can transfer by username
- [ ] Can transfer by public key
- [ ] Recipient receives funds automatically
- [ ] Change notes are handled correctly
- [ ] Can't send to yourself
- [ ] Pending notes excluded from balance

## Development

### Run Tests
```bash
# Backend tests
cargo test --package server

# Frontend tests
cd front && npm test
```

### Build for Production
```bash
# Backend
cargo build --release --package server

# Frontend
cd front && npm run build
```
