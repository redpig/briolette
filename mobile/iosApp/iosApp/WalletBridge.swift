import Foundation
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
