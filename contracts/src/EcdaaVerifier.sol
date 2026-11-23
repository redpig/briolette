// SPDX-License-Identifier: Apache-2.0
pragma solidity ^0.8.20;

/// @title ECDAA Verifier for BN254
/// @notice On-chain ECDAA signature verification using EIP-197 bn256 precompiles.
/// @dev Uses the alt_bn128 pairing precompile (0x08) for fraud proofs.
///      Only the pairing checks are performed on-chain; the Schnorr proof
///      verification is done off-chain and submitted as part of the fraud proof.
contract EcdaaVerifier {
    // Precompile addresses
    address constant BN256_ADD = address(0x06);
    address constant BN256_SCALAR_MUL = address(0x07);
    address constant BN256_PAIRING = address(0x08);

    /// @notice Verify the ECDAA pairing equations for a credential.
    /// @dev Checks:
    ///   1. e(A, Y) == e(B, P2)  =>  e(A, Y) * e(-B, P2) == 1
    ///   2. e(C, P2) == e(A+D, X)  =>  e(C, P2) * e(-(A+D), X) == 1
    /// @param a G1 point A from the signature (R in randomized form)
    /// @param b G1 point B from the signature (S in randomized form)
    /// @param c G1 point C from the signature (T in randomized form)
    /// @param d G1 point D from the signature (W in randomized form)
    /// @param gpkX G2 point X from the group public key
    /// @param gpkY G2 point Y from the group public key
    /// @return valid True if the pairing checks pass
    function verifyCredentialPairings(
        uint256[2] calldata a,
        uint256[2] calldata b,
        uint256[2] calldata c,
        uint256[2] calldata d,
        uint256[4] calldata gpkX,
        uint256[4] calldata gpkY
    ) external view returns (bool valid) {
        // P2 = G2 generator (standard alt_bn128 generator)
        // For alt_bn128: P2 = (
        //   11559732032986387107991004021392285783925812861821192530917403151452391805634,
        //   10857046999023057135944570762232829481370756359578518086990519993285655852781,
        //   4082367875863433681332203403145435568316851327593401208105741076214120093531,
        //   8495653923123431417604973247489272438418190587263600148770280649306958101930
        // )
        uint256[4] memory p2 = [
            uint256(11559732032986387107991004021392285783925812861821192530917403151452391805634),
            uint256(10857046999023057135944570762232829481370756359578518086990519993285655852781),
            uint256(4082367875863433681332203403145435568316851327593401208105741076214120093531),
            uint256(8495653923123431417604973247489272438418190587263600148770280649306958101930)
        ];

        // Check 1: e(A, Y) == e(B, P2)
        // Reformulated as: e(A, Y) * e(-B, P2) == 1
        // Negate B: -B = (B.x, p - B.y)
        uint256 pMod = 21888242871839275222246405745257275088696311157297823662689037894645226208583;
        uint256 negBY = pMod - b[1];

        // Pairing input: [A.x, A.y, Y.x_im, Y.x_re, Y.y_im, Y.y_re, -B.x, -B.y, P2...]
        bytes memory input1 = abi.encodePacked(
            a[0], a[1],
            gpkY[0], gpkY[1], gpkY[2], gpkY[3],
            b[0], negBY,
            p2[0], p2[1], p2[2], p2[3]
        );

        bool success;
        bytes memory result;
        (success, result) = BN256_PAIRING.staticcall(input1);
        if (!success || result.length < 32) return false;
        if (uint256(bytes32(result)) != 1) return false;

        // Check 2: e(C, P2) == e(A+D, X)
        // First compute A+D via the bn256Add precompile
        bytes memory addInput = abi.encodePacked(a[0], a[1], d[0], d[1]);
        bytes memory addResult;
        (success, addResult) = BN256_ADD.staticcall(addInput);
        if (!success || addResult.length < 64) return false;

        uint256 adX = uint256(bytes32(addResult));
        uint256 adY;
        assembly {
            adY := mload(add(addResult, 64))
        }

        // Negate (A+D)
        uint256 negAdY = pMod - adY;

        // e(C, P2) * e(-(A+D), X) == 1
        bytes memory input2 = abi.encodePacked(
            c[0], c[1],
            p2[0], p2[1], p2[2], p2[3],
            adX, negAdY,
            gpkX[0], gpkX[1], gpkX[2], gpkX[3]
        );

        (success, result) = BN256_PAIRING.staticcall(input2);
        if (!success || result.length < 32) return false;
        return uint256(bytes32(result)) == 1;
    }
}
