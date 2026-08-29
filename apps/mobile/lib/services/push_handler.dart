/// FCM/APNs handler stub — payload contains no message text.
///
/// Wire-up at final release:
/// 1. Add `firebase_messaging` to pubspec
/// 2. On background message `{ "type": "new_message", "sender_id": "..." }`
///    call [pollIncoming] and [syncMailbox]
library;

import '../src/rust/api.dart';

Future<void> handlePushData(Map<String, dynamic> data) async {
  if (data['type'] == 'new_message') {
    final senderId = data['sender_id'] as String?;
    if (senderId != null) {
      await syncMailbox(contactId: senderId);
    }
    await pollIncoming();
  }
}

PushPayloadDto examplePushPayload(String senderId) {
  return pushPayloadNewMessage(senderId: senderId);
}
