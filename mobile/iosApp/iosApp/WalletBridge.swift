import Foundation
import CryptoKit
import shared  // KMP shared framework

/// Swift implementation of the KMP `IosWalletDelegate` protocol.
///
/// Wraps the UniFFI-generated Rust FFI functions (`briolette.swift`)
/// and provides them to the Kotlin Compose UI via the delegate pattern.
///
/// Set up at app launch before the Compose UI is created.
class SwiftWalletDelegate: NSObject, IosWalletDelegate {

    func createWallet(
        name: String,
        registrarUri: String,
        clerkUri: String,
        mintUri: String,
        validateUri: String
    ) -> String {
        do {
            return try briolette.createWallet(
                name: name,
                registrarUri: registrarUri,
                clerkUri: clerkUri,
                mintUri: mintUri,
                validateUri: validateUri
            )
        } catch {
            return "{}"
        }
    }

    func createWalletWithAttestation(
        name: String,
        registrarUri: String,
        clerkUri: String,
        mintUri: String,
        validateUri: String,
        algorithm: Int32,
        signatureB64: String,
        publicKeyB64: String
    ) -> String {
        do {
            let attestation = briolette.AttestationData(
                algorithm: algorithm,
                signatureB64: signatureB64,
                publicKeyB64: publicKeyB64
            )
            return try briolette.createWalletWithAttestation(
                name: name,
                registrarUri: registrarUri,
                clerkUri: clerkUri,
                mintUri: mintUri,
                validateUri: validateUri,
                attestation: attestation
            )
        } catch {
            return "{}"
        }
    }

    func loadWallet(json: String) -> [String: Any?] {
        do {
            let state = try briolette.loadWallet(json: json)
            return stateToMap(state)
        } catch {
            return ["json": json, "walletName": "unknown", "whole": 0, "fractional": 0, "currency": "TEST", "tokenCount": 0, "ticketCount": 0]
        }
    }

    func saveWallet(stateJson: String) -> String {
        // The JSON is already the wallet state
        return stateJson
    }

    func synchronize(stateJson: String, clerkUri: String) -> [String: Any?] {
        do {
            let state = try briolette.loadWallet(json: stateJson)
            let result = try briolette.synchronize(state: state, clerkUri: clerkUri)
            return stateToMap(result)
        } catch {
            return errorState(stateJson)
        }
    }

    func requestTickets(stateJson: String, clerkUri: String, count: Int32) -> [String: Any?] {
        do {
            let state = try briolette.loadWallet(json: stateJson)
            let result = try briolette.requestTickets(state: state, clerkUri: clerkUri, count: UInt32(count))
            return stateToMap(result)
        } catch {
            return errorState(stateJson)
        }
    }

    func withdraw(stateJson: String, mintUri: String, amount: Int32) -> [String: Any?] {
        do {
            let state = try briolette.loadWallet(json: stateJson)
            let result = try briolette.withdraw(state: state, mintUri: mintUri, amount: UInt32(amount))
            return stateToMap(result)
        } catch {
            return errorState(stateJson)
        }
    }

    func transferTokens(stateJson: String, recipientTicketB64: String, amount: Int32) -> [String: Any?] {
        do {
            let state = try briolette.loadWallet(json: stateJson)
            let result = try briolette.transferTokens(state: state, recipientTicketB64: recipientTicketB64, amount: UInt32(amount))
            return [
                "state": stateToMap(result.state),
                "tokensB64": result.tokensB64,
            ]
        } catch {
            return ["state": errorState(stateJson), "tokensB64": [String]()]
        }
    }

    func receiveTokens(stateJson: String, tokensB64: [String]) -> [String: Any?] {
        do {
            let state = try briolette.loadWallet(json: stateJson)
            let result = try briolette.receiveTokens(state: state, tokensB64: tokensB64)
            return stateToMap(result)
        } catch {
            return errorState(stateJson)
        }
    }

    func validateTokens(stateJson: String, validateUri: String) -> [String: Any?] {
        do {
            let state = try briolette.loadWallet(json: stateJson)
            let result = try briolette.validateTokens(state: state, validateUri: validateUri)
            return [
                "state": stateToMap(result.state),
                "allValid": result.allValid,
                "validCount": result.validCount,
                "invalidCount": result.invalidCount,
            ]
        } catch {
            return ["state": errorState(stateJson), "allValid": false, "validCount": 0, "invalidCount": 0]
        }
    }

    func getReceivingTicketB64(stateJson: String) -> String {
        do {
            let state = try briolette.loadWallet(json: stateJson)
            return try briolette.getReceivingTicketB64(state: state)
        } catch {
            return ""
        }
    }

    func getBalance(stateJson: String) -> [String: Any?] {
        do {
            let state = try briolette.loadWallet(json: stateJson)
            let balance = briolette.getBalance(state: state)
            return [
                "whole": balance.whole,
                "fractional": balance.fractional,
                "currency": balance.currency,
                "tokenCount": balance.tokenCount,
            ]
        } catch {
            return ["whole": 0, "fractional": 0, "currency": "TEST", "tokenCount": 0]
        }
    }

    func getTicketCount(stateJson: String) -> Int32 {
        do {
            let state = try briolette.loadWallet(json: stateJson)
            return Int32(briolette.getTicketCount(state: state))
        } catch {
            return 0
        }
    }

    func generateAttestationWithPreimage(preimageB64: String) -> [String: Any?] {
        // Decode the base64 preimage and SHA-256 hash it for the challenge.
        guard let preimageData = Data(base64Encoded: preimageB64) else {
            return [:]
        }
        let challengeData = SHA256.hash(data: preimageData)
        let challenge = Data(challengeData)

        if #available(iOS 14.0, *) {
            let helper = AppAttestHelper()
            guard helper.isSupported else {
                return [:]
            }
            let semaphore = DispatchSemaphore(value: 0)
            var result: [String: Any?] = [:]
            Task {
                do {
                    let attestation = try await helper.generateAttestation(challenge: challenge)
                    result = [
                        "algorithm": attestation.algorithm,
                        "signatureB64": attestation.signatureB64,
                        "publicKeyB64": attestation.publicKeyB64,
                    ]
                } catch {
                    result = [:]
                }
                semaphore.signal()
            }
            semaphore.wait()
            return result
        } else {
            return [:]
        }
    }

    func initWalletKeys(
        name: String,
        registrarUri: String,
        clerkUri: String,
        mintUri: String,
        validateUri: String
    ) -> [String: Any?] {
        do {
            let result = try briolette.initWalletKeys(
                name: name,
                registrarUri: registrarUri,
                clerkUri: clerkUri,
                mintUri: mintUri,
                validateUri: validateUri
            )
            return [
                "walletJson": result.walletJson,
                "challengePreimageB64": result.challengePreimageB64,
            ]
        } catch {
            return [:]
        }
    }

    func registerWalletWithAttestation(
        walletJson: String,
        algorithm: Int32,
        signatureB64: String,
        publicKeyB64: String,
        nacCardPublicKeyB64: String,
        ttcCardPublicKeyB64: String,
        cardAttestationB64: String
    ) -> String {
        do {
            let attestation = briolette.AttestationData(
                algorithm: algorithm,
                signatureB64: signatureB64,
                publicKeyB64: publicKeyB64
            )
            return try briolette.registerWalletWithAttestation(
                walletJson: walletJson,
                attestation: attestation,
                nacCardPublicKeyB64: nacCardPublicKeyB64,
                ttcCardPublicKeyB64: ttcCardPublicKeyB64,
                cardAttestationB64: cardAttestationB64
            )
        } catch {
            return "{}"
        }
    }

    func splitKeyStart(
        name: String,
        registrarUri: String,
        clerkUri: String,
        mintUri: String,
        validateUri: String
    ) -> [String: Any?] {
        do {
            let result = try briolette.splitKeyStart(
                name: name,
                registrarUri: registrarUri,
                clerkUri: clerkUri,
                mintUri: mintUri,
                validateUri: validateUri
            )
            return [
                "stateJson": result.stateJson,
                "bTtcB64": result.bTtcB64,
            ]
        } catch {
            return [:]
        }
    }

    func splitKeyAfterTtcCommit(
        stateJson: String, qCardTtcB64: String, uCardTtcB64: String
    ) -> [String: Any?] {
        do {
            let result = try briolette.splitKeyAfterTtcCommit(
                stateJson: stateJson,
                qCardTtcB64: qCardTtcB64,
                uCardTtcB64: uCardTtcB64
            )
            return [
                "stateJson": result.stateJson,
                "cTtcB64": result.cTtcB64,
                "bNacB64": result.bNacB64,
            ]
        } catch {
            return [:]
        }
    }

    func splitKeyAfterNacCommit(
        stateJson: String, qCardNacB64: String, uCardNacB64: String
    ) -> [String: Any?] {
        do {
            let result = try briolette.splitKeyAfterNacCommit(
                stateJson: stateJson,
                qCardNacB64: qCardNacB64,
                uCardNacB64: uCardNacB64
            )
            return [
                "stateJson": result.stateJson,
                "cNacB64": result.cNacB64,
            ]
        } catch {
            return [:]
        }
    }

    func splitKeyComplete(
        stateJson: String, sCardTtcB64: String, sCardNacB64: String
    ) -> [String: Any?] {
        do {
            let result = try briolette.splitKeyComplete(
                stateJson: stateJson,
                sCardTtcB64: sCardTtcB64,
                sCardNacB64: sCardNacB64
            )
            return [
                "walletJson": result.walletJson,
                "challengePreimageB64": result.challengePreimageB64,
                "nacCardPublicKeyB64": result.nacCardPublicKeyB64,
                "ttcCardPublicKeyB64": result.ttcCardPublicKeyB64,
            ]
        } catch {
            return [:]
        }
    }

    // MARK: - Helpers

    private func stateToMap(_ state: briolette.WalletState) -> [String: Any?] {
        return [
            "json": state.json,
            "whole": state.balance.whole,
            "fractional": state.balance.fractional,
            "currency": state.balance.currency,
            "tokenCount": state.balance.tokenCount,
            "ticketCount": state.ticketCount,
            "walletName": state.walletName,
        ]
    }

    private func errorState(_ json: String) -> [String: Any?] {
        return ["json": json, "walletName": "unknown", "whole": 0, "fractional": 0, "currency": "TEST", "tokenCount": 0, "ticketCount": 0]
    }
}

/// Install the Swift wallet delegate into the KMP bridge before UI creation.
func installWalletBridge() {
    let delegate = SwiftWalletDelegate()
    IosWalletBridge.Companion.shared.setDelegate(delegate: delegate)
}

// Note: The UniFFI-generated functions (createWallet, loadWallet, etc.) are
// defined as top-level functions in generated/briolette.swift and are compiled
// into this same target — no additional import is needed.
