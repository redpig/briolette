# Credentials in Briolette

There are a number of credentials present throughout briolette, each with its
own capabilities.


## Service certificates

All operator services are expected to operate on top of a normal TLS deployment
where the client can rely on server authenticity based on the certificates and keys
held by the server.

None of briolette's trust depends on TLS functioning, it does provide heightened
assurance against denial of service or service disruption attacks.

## State signing key

The state signing key is the most critical cryptographic key in the system.
After a system-defined period of time has elapsed, a 'epoch', briolette
system-wide state is updated.  This update includes the current epoch number (a
monotonically increasing 64-bit integer, usually based on wall-clock time), any
ticket group numbers that have been temporarily blocked from the system
(revoked), and the other public keys for the remainder of the system that a
client, or wallet, device must know: token transfer public key, ticket server
public key, issuing mint public keys, etc.

The level of trust given to the state signing key enables a compromise between
in-the-field key rotation with minimal gossip overhead. A secondary signature
over the extended state (epoch) data may be appropriate to introduce if the
risk introduced is too high.

### Installation

The initial state signing key is installed on the wallet device by the wallet
vendor or through an out-of-band installation process.  Knowledge of the state
server (clerk) and the state signing key is all that is required for a
certified wallet to participate in different briolette systems.

See discussion of the 'Network Access Credential' for more.

### Usage

The state signing key does not need to be online and may be air gapped. The
data it signs is fixed and well-formed, so the process for using the signing
key, or keys, may be accompanied by additional software validation both on and
off the signing infrastructure.  The signer operates over the next epoch, a
bitfield of revoked groups, and the extended epoch data cryptographic hash.
The extended epoch data contains the all other signing keys, as well as the
alternative state signing keys.

Epoch updates are not expected in real-time and as such, the generation of
the new epoch data and signing process may be subject to high levels of
assurance.

## Network Access Credential (NAC) Registrar Key

Wallet vendors will have at least one NAC issuer key. This key is used to grant
network access credentials to wallets. The associated group public key must be
known and accepted by briolette system operators for the credential to be used.

The issuer may require the wallet to have a proprietary key or perform some
other service to be accepted for credential issuance.

### Tiered NAC issuers and attestation

The registrar maintains per-security-level NAC issuer keypairs (Low, Medium,
High).  During registration, the registrar verifies hardware attestation
(Android Key Attestation or Apple App Attest) and maps the result to a
security level:

- **Software / no attestation** → Low
- **TEE or StrongBox / App Attest** → Medium
- **Medium + valid split-key proof** (smartcard contribution) → High

Each level has its own NAC group public key.  This means the clerk can apply
different ticket policies per NAC group without knowing the attestation details.
The `GroupPolicy` entries in `ExtendedEpochData` map each NAC group public key
to a ticket lifetime (in epochs).  Lower-assurance groups receive shorter
lifetimes; the lowest tier may be restricted to online-only (lifetime 0 or 1).

All wallets share the same TTC group so they can transact with each other.  The
differentiation is purely on the NAC (policy) side — merchants only see tickets
with varying expiry, never the underlying security rationale.

## Network Access Credential (NAC)

The NAC is used by a wallet to connect to briolette operator services and is
required for acquiring a token transfer credential.

Signatures over requests with epoch-bound basenames may be used to create
linkable signatures over time periods.  This will allow operator services to
limit requests from any given wallet during a time period without being able
to uniquely identify that wallet in the future.

## Token Transfer Credential (TTC) Registrar Key

This key is usually held by the system operator and is used to grant token
transfer credentials to wallets.  Wallets will need to request a TTC upon
setup for a given operator and its request must be authenticated by the
wallet's NAC with a known NAC group public key.

## Token Transfer Credential (TTC)

The TTC is used by the wallet to send and receive tokens.  The wallet holds the
private key and the credential acts as a "public" key.  The credential itself
is never used directly.  Instead it may be randomized prior to use.

Prior to transacting, a wallet must pre-randomize its credential several times.
It will take these randomized credentials and present them to the ticket clerk
service (signed with the wallet's NAC).  The ticket clerk will return signed
tickets which may be used as the destination to receive tokens at.

When transferring received tokens, the wallet must use the same randomized
credential from the signed ticket the token was transferred to when signing the
transaction.

## Token Signing Key

The token signing key is the minting key.  It fixates the token descriptor data
with its signature and assigns the first recipient of a token.

## Transfer Ticket Signing Key

This key is held by the ticket clerk service and is used to sign transfer tickets
which are built from randomized TTCs and specific policy attributes, such as
expiration times.

## Trust bootstrap: app → registrar → system

The wallet's trust root is its network registrar.  The app is configured with
the registrar's address (currently via user input; in production this would be
baked in by the wallet vendor or discovered via a well-known URI).  During
registration (`RegisterCall`), the registrar returns `CredentialReply` messages
for both the NAC and TTC.  Each reply includes the issuer's `group_public_key`,
giving the wallet everything it needs to participate in the system:

1. The **NAC group public key** — proves to operator services that the wallet
   is legitimate and at what assurance tier.
2. The **TTC group public key** — used to verify that transfer credentials and
   tickets belong to the correct system.

The wallet then connects to the clerk (whose address is provided by the
registrar or discovered via the `ServiceMap` in `ExtendedEpochData`) and
fetches its first `EpochUpdate`.  This update, signed by the epoch signing key,
delivers the complete trust bundle: all TTC group public keys, epoch signing
keys, ticket signing keys, mint signing keys, and service URIs.  On first
contact the wallet trusts the epoch signing keys it receives, which is
acceptable because it is bootstrapping from a registrar it already trusts.

## Key rotation

All key types in the system are rotatable via the `ExtendedEpochData` message
distributed through signed `EpochUpdate`s.  Each key field is `repeated`,
allowing the operator to publish overlapping key sets during a transition:

- `ttc_group_public_keys` — token transfer credential group keys
- `epoch_signing_keys` — keys that sign epoch updates themselves
- `ticket_signing_keys` — keys the clerk uses to sign tickets
- `mint_signing_keys` — keys the mint uses to sign token bases

The rotation procedure is:

1. **Overlap**: Add the new key alongside the old key in one epoch.
2. **Migrate**: Start signing with the new key.  Wallets accept signatures
   from any key in the list, so both old and new are valid.
3. **Retire**: Drop the old key in a subsequent epoch once all wallets have
   updated.

The wallet validates signatures by checking the signing key against its
current key list.  If the key is not recognized, the operation is rejected.
This means key rotation propagates naturally through the epoch update
mechanism with no special protocol messages required.

NAC issuer key rotation follows a similar pattern but is managed at the
registrar level.  The registrar can begin issuing credentials under a new NAC
group key while the clerk continues to accept tickets from wallets holding
credentials from the old key.  The old NAC group key remains in the clerk's
`GroupPolicy` list until all wallets have re-registered.


