//! runnerからの通知。HTTP/SSEの送信やCLIの表示は呼び出し側に任せる。
use std::fmt;

use crate::contracts::{EventPayload, OperationPlan, OperationState};

/// 通知先の切断はrunnerの中止要求ではない。
/// 実装は処理を長時間ブロックせず、通知の成否をDB処理の成否にしない。
/// runnerはSQL成功後だけcommittedを通知する。
pub trait Reporter: Send + Sync {
    fn report(&self, event: EventPayload);

    /// 実行前の予定。接続情報を含まない。
    fn plan(&self, _operations: &[OperationPlan]) {}

    /// 実際に送るSQL。setup/seedの各SQLは同じoperation_idに紐づく。
    fn statement(&self, _operation: &OperationPlan) {}

    /// seed操作を増やさず、成功したINSERTまでの累積件数を知らせる。
    fn seed_progress(&self, _operation: &OperationState) {}

    /// 既存CLI向けの表示。画面側は構造化通知を使い、この文字列は解析しない。
    fn line(&self, _message: fmt::Arguments<'_>) {}
}
