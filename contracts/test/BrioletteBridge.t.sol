// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.20;

import "../src/BrioletteBridge.sol";
import "../src/EcdaaVerifier.sol";

/// @title BrioletteBridge Test Suite
/// @notice Foundry tests for the Briolette L2 bridge contract.
/// @dev Run with: forge test
contract BrioletteBridgeTest {
    BrioletteBridge public bridge;
    address public operator;

    // Foundry test lifecycle
    function setUp() public {
        operator = address(this);
        bridge = new BrioletteBridge(operator);
    }

    // ========================================================================
    // Deposit tests
    // ========================================================================

    function testDeposit() public {
        bytes32 ticketHash = keccak256("test-ticket");
        uint256 balBefore = address(bridge).balance;

        bridge.deposit{value: 1 ether}(ticketHash);

        // Check state
        (
            address depositor,
            uint256 amount,
            bytes32 storedHash,
            uint256 timestamp,
            bool processed
        ) = bridge.deposits(0);

        require(depositor == address(this), "wrong depositor");
        require(amount == 1 ether, "wrong amount");
        require(storedHash == ticketHash, "wrong ticket hash");
        require(!processed, "should not be processed");
        require(
            address(bridge).balance == balBefore + 1 ether,
            "wrong balance"
        );
        require(bridge.totalDeposited() == 1 ether, "wrong totalDeposited");
    }

    function testDepositZeroValueReverts() public {
        bytes32 ticketHash = keccak256("test-ticket");
        (bool success, ) = address(bridge).call{value: 0}(
            abi.encodeWithSelector(bridge.deposit.selector, ticketHash)
        );
        require(!success, "should revert on zero deposit");
    }

    function testDepositZeroTicketReverts() public {
        (bool success, ) = address(bridge).call{value: 1 ether}(
            abi.encodeWithSelector(bridge.deposit.selector, bytes32(0))
        );
        require(!success, "should revert on zero ticket");
    }

    function testMarkDepositProcessed() public {
        bytes32 ticketHash = keccak256("test-ticket");
        bridge.deposit{value: 1 ether}(ticketHash);

        bridge.markDepositProcessed(0);

        (, , , , bool processed) = bridge.deposits(0);
        require(processed, "should be processed");
        require(bridge.totalMinted() == 1 ether, "wrong totalMinted");
    }

    function testMarkDepositProcessedTwiceReverts() public {
        bytes32 ticketHash = keccak256("test-ticket");
        bridge.deposit{value: 1 ether}(ticketHash);
        bridge.markDepositProcessed(0);

        (bool success, ) = address(bridge).call(
            abi.encodeWithSelector(bridge.markDepositProcessed.selector, 0)
        );
        require(!success, "should revert on double process");
    }

    // ========================================================================
    // Epoch tests
    // ========================================================================

    function testPublishEpoch() public {
        bytes32 dataHash = keccak256("epoch-data-1");
        bridge.publishEpoch(1, dataHash);

        (bytes32 storedHash, uint256 timestamp, bool challenged) = bridge.epochs(1);
        require(storedHash == dataHash, "wrong data hash");
        require(timestamp > 0, "timestamp not set");
        require(!challenged, "should not be challenged");
        require(bridge.latestEpoch() == 1, "wrong latestEpoch");
    }

    function testPublishEpochZeroHashReverts() public {
        (bool success, ) = address(bridge).call(
            abi.encodeWithSelector(bridge.publishEpoch.selector, uint64(1), bytes32(0))
        );
        require(!success, "should revert on zero hash");
    }

    function testPublishEpochNotNewerReverts() public {
        bridge.publishEpoch(5, keccak256("data-5"));

        (bool success, ) = address(bridge).call(
            abi.encodeWithSelector(
                bridge.publishEpoch.selector,
                uint64(3),
                keccak256("data-3")
            )
        );
        require(!success, "should revert on older epoch");
    }

    // ========================================================================
    // Withdrawal tests
    // ========================================================================

    function testInitiateWithdrawal() public {
        // First deposit to fund the bridge
        bridge.deposit{value: 2 ether}(keccak256("ticket"));
        bridge.markDepositProcessed(0);

        uint256 wid = bridge.initiateWithdrawal(
            payable(address(0xBEEF)),
            1 ether
        );

        (
            address recipient,
            uint256 amount,
            uint256 initiatedAt,
            bool completed,
            bool challenged
        ) = bridge.withdrawals(wid);

        require(recipient == address(0xBEEF), "wrong recipient");
        require(amount == 1 ether, "wrong amount");
        require(initiatedAt > 0, "timestamp not set");
        require(!completed, "should not be completed");
        require(!challenged, "should not be challenged");
    }

    function testInitiateWithdrawalZeroAmountReverts() public {
        bridge.deposit{value: 1 ether}(keccak256("ticket"));

        (bool success, ) = address(bridge).call(
            abi.encodeWithSelector(
                bridge.initiateWithdrawal.selector,
                payable(address(0xBEEF)),
                0
            )
        );
        require(!success, "should revert on zero amount");
    }

    // ========================================================================
    // Admin tests
    // ========================================================================

    function testTransferOperator() public {
        address newOp = address(0xCAFE);
        bridge.transferOperator(newOp);
        require(bridge.operator() == newOp, "operator not transferred");
    }

    function testTransferOperatorZeroReverts() public {
        (bool success, ) = address(bridge).call(
            abi.encodeWithSelector(bridge.transferOperator.selector, address(0))
        );
        require(!success, "should revert on zero operator");
    }

    function testBridgeBalance() public {
        bridge.deposit{value: 3 ether}(keccak256("ticket"));
        require(bridge.bridgeBalance() == 3 ether, "wrong bridge balance");
    }

    // Allow this contract to receive ETH (for test callbacks)
    receive() external payable {}
}
