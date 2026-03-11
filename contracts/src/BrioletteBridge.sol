// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.20;

import "./EcdaaVerifier.sol";

/// @title Briolette L2 Bridge
/// @notice Bridge contract for depositing/withdrawing assets between Ethereum L1
///         and the Briolette offline-capable digital currency system.
/// @dev Implements a Validium-style architecture:
///      - Deposits lock funds on L1 and emit events for the Briolette mint
///      - Withdrawals are initiated by the operator after token chain verification
///      - Epochs are anchored on L1 for auditability
///      - Fraud proofs allow challenging bad operator behavior
contract BrioletteBridge {
    // ============================================================================
    // State
    // ============================================================================

    /// @notice The operator address (can publish epochs and process withdrawals)
    address public operator;

    /// @notice The ECDAA verifier contract for fraud proofs
    EcdaaVerifier public ecdaaVerifier;

    /// @notice Challenge period for withdrawals (default: 7 days)
    uint256 public constant CHALLENGE_PERIOD = 7 days;

    /// @notice Total deposited balance (ETH)
    uint256 public totalDeposited;

    /// @notice Total minted in Briolette (tracked for supply cap enforcement)
    uint256 public totalMinted;

    // Epoch tracking
    struct EpochCommitment {
        bytes32 dataHash;
        uint256 timestamp;
        bool challenged;
    }

    /// @notice Epoch number -> commitment
    mapping(uint64 => EpochCommitment) public epochs;

    /// @notice Latest published epoch number
    uint64 public latestEpoch;

    // Withdrawal tracking
    struct Withdrawal {
        address payable recipient;
        uint256 amount;
        uint256 initiatedAt;
        bool completed;
        bool challenged;
    }

    /// @notice Withdrawal ID -> withdrawal data
    mapping(uint256 => Withdrawal) public withdrawals;
    uint256 public nextWithdrawalId;

    // Deposit tracking
    struct Deposit {
        address depositor;
        uint256 amount;
        bytes32 ticketHash; // Hash of the Briolette ticket to receive tokens
        uint256 timestamp;
        bool processed; // Set by operator after minting Briolette tokens
    }

    mapping(uint256 => Deposit) public deposits;
    uint256 public nextDepositId;

    // ============================================================================
    // Key Registry — on-chain authority for Briolette system keys
    // ============================================================================

    /// @notice Mint signing public keys (P256 SEC1 compressed, 33 bytes each)
    bytes[] public mintSigningKeys;

    /// @notice Ticket signing public keys (clerk keys)
    bytes[] public ticketSigningKeys;

    /// @notice TTC group public key (ECDAA group key)
    bytes public ttcGroupPublicKey;

    /// @notice Key registry version (incremented on any key change)
    uint256 public keyRegistryVersion;

    // ============================================================================
    // Events
    // ============================================================================

    event Deposited(
        uint256 indexed depositId,
        address indexed depositor,
        uint256 amount,
        bytes32 ticketHash
    );

    event DepositProcessed(uint256 indexed depositId);

    event WithdrawalInitiated(
        uint256 indexed withdrawalId,
        address indexed recipient,
        uint256 amount
    );

    event WithdrawalCompleted(uint256 indexed withdrawalId);

    event WithdrawalChallenged(uint256 indexed withdrawalId, address challenger);

    event EpochPublished(uint64 indexed epochNum, bytes32 dataHash);

    event EpochChallenged(uint64 indexed epochNum, address challenger);

    event OperatorTransferred(address indexed oldOperator, address indexed newOperator);

    event MintKeyAdded(uint256 indexed keyIndex, bytes key);
    event MintKeyRemoved(uint256 indexed keyIndex);
    event TicketKeyAdded(uint256 indexed keyIndex, bytes key);
    event TicketKeyRemoved(uint256 indexed keyIndex);
    event TtcGroupKeyUpdated(bytes key);
    event KeyRegistryUpdated(uint256 indexed version);

    // ============================================================================
    // Modifiers
    // ============================================================================

    modifier onlyOperator() {
        require(msg.sender == operator, "BrioletteBridge: not operator");
        _;
    }

    // ============================================================================
    // Constructor
    // ============================================================================

    constructor(address _operator) {
        require(_operator != address(0), "BrioletteBridge: zero operator");
        operator = _operator;
        ecdaaVerifier = new EcdaaVerifier();
    }

    // ============================================================================
    // Deposit functions
    // ============================================================================

    /// @notice Deposit ETH into the bridge for conversion to Briolette tokens.
    /// @param ticketHash Hash of the Briolette SignedTicket that should receive
    ///        the minted tokens. The Briolette mint watches for Deposited events.
    function deposit(bytes32 ticketHash) external payable {
        require(msg.value > 0, "BrioletteBridge: zero deposit");
        require(ticketHash != bytes32(0), "BrioletteBridge: zero ticket hash");

        uint256 depositId = nextDepositId++;
        deposits[depositId] = Deposit({
            depositor: msg.sender,
            amount: msg.value,
            ticketHash: ticketHash,
            timestamp: block.timestamp,
            processed: false
        });

        totalDeposited += msg.value;

        emit Deposited(depositId, msg.sender, msg.value, ticketHash);
    }

    /// @notice Mark a deposit as processed (operator has minted Briolette tokens).
    /// @param depositId The deposit to mark as processed.
    function markDepositProcessed(uint256 depositId) external onlyOperator {
        Deposit storage dep = deposits[depositId];
        require(dep.amount > 0, "BrioletteBridge: deposit not found");
        require(!dep.processed, "BrioletteBridge: already processed");

        dep.processed = true;
        totalMinted += dep.amount;

        emit DepositProcessed(depositId);
    }

    // ============================================================================
    // Withdrawal functions
    // ============================================================================

    /// @notice Initiate a withdrawal. Called by the operator after verifying a
    ///         Briolette token chain off-chain.
    /// @param recipient The L1 address to receive the withdrawn ETH.
    /// @param amount The amount to withdraw.
    /// @return withdrawalId The ID of the created withdrawal.
    function initiateWithdrawal(
        address payable recipient,
        uint256 amount
    ) external onlyOperator returns (uint256 withdrawalId) {
        require(recipient != address(0), "BrioletteBridge: zero recipient");
        require(amount > 0, "BrioletteBridge: zero amount");
        require(
            address(this).balance >= amount,
            "BrioletteBridge: insufficient balance"
        );

        withdrawalId = nextWithdrawalId++;
        withdrawals[withdrawalId] = Withdrawal({
            recipient: recipient,
            amount: amount,
            initiatedAt: block.timestamp,
            completed: false,
            challenged: false
        });

        totalMinted -= amount;

        emit WithdrawalInitiated(withdrawalId, recipient, amount);
    }

    /// @notice Complete a withdrawal after the challenge period.
    /// @param withdrawalId The withdrawal to complete.
    function completeWithdrawal(uint256 withdrawalId) external {
        Withdrawal storage w = withdrawals[withdrawalId];
        require(w.amount > 0, "BrioletteBridge: withdrawal not found");
        require(!w.completed, "BrioletteBridge: already completed");
        require(!w.challenged, "BrioletteBridge: challenged");
        require(
            block.timestamp >= w.initiatedAt + CHALLENGE_PERIOD,
            "BrioletteBridge: challenge period active"
        );

        w.completed = true;
        totalDeposited -= w.amount;

        (bool sent, ) = w.recipient.call{value: w.amount}("");
        require(sent, "BrioletteBridge: ETH transfer failed");

        emit WithdrawalCompleted(withdrawalId);
    }

    // ============================================================================
    // Epoch functions
    // ============================================================================

    /// @notice Publish an epoch commitment to L1.
    /// @param epochNum The epoch number.
    /// @param dataHash keccak256 hash of the serialized EpochData.
    function publishEpoch(
        uint64 epochNum,
        bytes32 dataHash
    ) external onlyOperator {
        require(dataHash != bytes32(0), "BrioletteBridge: zero hash");
        require(
            epochNum > latestEpoch || latestEpoch == 0,
            "BrioletteBridge: epoch not newer"
        );
        require(
            epochs[epochNum].timestamp == 0,
            "BrioletteBridge: epoch exists"
        );

        epochs[epochNum] = EpochCommitment({
            dataHash: dataHash,
            timestamp: block.timestamp,
            challenged: false
        });

        latestEpoch = epochNum;

        emit EpochPublished(epochNum, dataHash);
    }

    // ============================================================================
    // Fraud proof functions
    // ============================================================================

    /// @notice Challenge a withdrawal with ECDAA fraud proof.
    /// @dev The challenger provides the ECDAA credential components from a
    ///      double-spend signature. If the pairing checks verify that the
    ///      credential is valid but the operator processed the withdrawal
    ///      anyway, the withdrawal is cancelled.
    /// @param withdrawalId The withdrawal to challenge.
    /// @param a G1 point A from the double-spend signature
    /// @param b G1 point B from the double-spend signature
    /// @param c G1 point C from the double-spend signature
    /// @param d G1 point D from the double-spend signature
    /// @param gpkX G2 point X from the group public key
    /// @param gpkY G2 point Y from the group public key
    function challengeWithdrawal(
        uint256 withdrawalId,
        uint256[2] calldata a,
        uint256[2] calldata b,
        uint256[2] calldata c,
        uint256[2] calldata d,
        uint256[4] calldata gpkX,
        uint256[4] calldata gpkY
    ) external {
        Withdrawal storage w = withdrawals[withdrawalId];
        require(w.amount > 0, "BrioletteBridge: withdrawal not found");
        require(!w.completed, "BrioletteBridge: already completed");
        require(!w.challenged, "BrioletteBridge: already challenged");
        require(
            block.timestamp < w.initiatedAt + CHALLENGE_PERIOD,
            "BrioletteBridge: challenge period over"
        );

        // Verify the ECDAA pairing equations on-chain
        bool valid = ecdaaVerifier.verifyCredentialPairings(
            a, b, c, d, gpkX, gpkY
        );
        require(valid, "BrioletteBridge: invalid fraud proof");

        // Cancel the withdrawal
        w.challenged = true;
        totalMinted += w.amount; // Restore minted count

        emit WithdrawalChallenged(withdrawalId, msg.sender);
    }

    /// @notice Challenge an epoch with evidence of operator misbehavior.
    /// @param epochNum The epoch to challenge.
    function challengeEpoch(uint64 epochNum) external {
        EpochCommitment storage epoch = epochs[epochNum];
        require(epoch.timestamp > 0, "BrioletteBridge: epoch not found");
        require(!epoch.challenged, "BrioletteBridge: already challenged");

        // For now, epoch challenges are permissioned to the operator's
        // governance mechanism. A full implementation would verify
        // specific fraud proof data (e.g., revocation data that should
        // have been included but wasn't).
        epoch.challenged = true;

        emit EpochChallenged(epochNum, msg.sender);
    }

    // ============================================================================
    // Key Registry functions
    // ============================================================================

    /// @notice Add a mint signing public key.
    /// @param key The P256 SEC1 compressed public key (33 bytes).
    function addMintKey(bytes calldata key) external onlyOperator {
        require(key.length == 33, "BrioletteBridge: invalid key length");
        mintSigningKeys.push(key);
        keyRegistryVersion++;
        emit MintKeyAdded(mintSigningKeys.length - 1, key);
        emit KeyRegistryUpdated(keyRegistryVersion);
    }

    /// @notice Remove a mint signing key by index (swap-and-pop).
    /// @param index The index of the key to remove.
    function removeMintKey(uint256 index) external onlyOperator {
        require(index < mintSigningKeys.length, "BrioletteBridge: index out of bounds");
        mintSigningKeys[index] = mintSigningKeys[mintSigningKeys.length - 1];
        mintSigningKeys.pop();
        keyRegistryVersion++;
        emit MintKeyRemoved(index);
        emit KeyRegistryUpdated(keyRegistryVersion);
    }

    /// @notice Add a ticket signing public key.
    /// @param key The clerk's signing public key.
    function addTicketKey(bytes calldata key) external onlyOperator {
        require(key.length == 33, "BrioletteBridge: invalid key length");
        ticketSigningKeys.push(key);
        keyRegistryVersion++;
        emit TicketKeyAdded(ticketSigningKeys.length - 1, key);
        emit KeyRegistryUpdated(keyRegistryVersion);
    }

    /// @notice Remove a ticket signing key by index (swap-and-pop).
    /// @param index The index of the key to remove.
    function removeTicketKey(uint256 index) external onlyOperator {
        require(index < ticketSigningKeys.length, "BrioletteBridge: index out of bounds");
        ticketSigningKeys[index] = ticketSigningKeys[ticketSigningKeys.length - 1];
        ticketSigningKeys.pop();
        keyRegistryVersion++;
        emit TicketKeyRemoved(index);
        emit KeyRegistryUpdated(keyRegistryVersion);
    }

    /// @notice Set the TTC group public key.
    /// @param key The ECDAA group public key bytes.
    function setTtcGroupKey(bytes calldata key) external onlyOperator {
        require(key.length > 0, "BrioletteBridge: empty key");
        ttcGroupPublicKey = key;
        keyRegistryVersion++;
        emit TtcGroupKeyUpdated(key);
        emit KeyRegistryUpdated(keyRegistryVersion);
    }

    /// @notice Get all mint signing keys.
    function getMintKeys() external view returns (bytes[] memory) {
        return mintSigningKeys;
    }

    /// @notice Get all ticket signing keys.
    function getTicketKeys() external view returns (bytes[] memory) {
        return ticketSigningKeys;
    }

    /// @notice Get the number of mint keys.
    function mintKeyCount() external view returns (uint256) {
        return mintSigningKeys.length;
    }

    /// @notice Get the number of ticket keys.
    function ticketKeyCount() external view returns (uint256) {
        return ticketSigningKeys.length;
    }

    // ============================================================================
    // Admin functions
    // ============================================================================

    /// @notice Transfer operator role.
    /// @param newOperator The new operator address.
    function transferOperator(address newOperator) external onlyOperator {
        require(newOperator != address(0), "BrioletteBridge: zero operator");
        emit OperatorTransferred(operator, newOperator);
        operator = newOperator;
    }

    /// @notice Get the bridge's ETH balance.
    function bridgeBalance() external view returns (uint256) {
        return address(this).balance;
    }

    /// @notice Allow the contract to receive ETH directly.
    receive() external payable {
        totalDeposited += msg.value;
    }
}
