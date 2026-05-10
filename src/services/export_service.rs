use crate::domain::settlement::SettlementSnapshot;
use crate::domain::export::format_money;
use crate::error::ExportError;

pub struct ExportService {}

impl ExportService {
    pub fn new() -> Self {
        Self {}
    }

    pub fn export_user_bills(&self, snapshot: &SettlementSnapshot) -> Result<Vec<u8>, ExportError> {
        let mut wtr = csv::Writer::from_writer(vec![]);
        wtr.write_record(["用户ID", "昵称", "商品明细", "拼团原价", "单领原价", "优惠分摊", "赠品抵扣", "邮费", "最终应付", "付款状态"])?;

        for bill in &snapshot.user_bills {
            let detail = bill.lines.iter()
                .map(|l| format!("{} x{}", l.item_name, l.quantity))
                .collect::<Vec<_>>()
                .join("；");

            let split_gross: i64 = bill.lines.iter()
                .filter(|l| l.kind == "split")
                .map(|l| l.gross.0)
                .sum();
            let single_gross: i64 = bill.lines.iter()
                .filter(|l| l.kind == "single")
                .map(|l| l.gross.0)
                .sum();

            let payment_status_str = match bill.payment_status {
                crate::domain::settlement::PaymentStatus::Unpaid => "未付款",
                crate::domain::settlement::PaymentStatus::Paid => "已付款",
                crate::domain::settlement::PaymentStatus::Partial => "部分付款",
                crate::domain::settlement::PaymentStatus::Refunded => "已退款",
            };

            wtr.write_record([
                bill.user_id.0.as_str(),
                bill.display_name.as_str(),
                detail.as_str(),
                &format_money(crate::domain::money::MoneyCents(split_gross)),
                &format_money(crate::domain::money::MoneyCents(single_gross)),
                &format_money(bill.discount_share),
                &format_money(bill.gift_value_share),
                &format_money(bill.shipping_fee),
                &format_money(bill.final_total),
                payment_status_str,
            ])?;
        }

        wtr.flush()?;
        Ok(wtr.into_inner().map_err(|e| ExportError::Io(e.into_error()))?)
    }

    pub fn export_item_summary(&self, snapshot: &SettlementSnapshot) -> Result<Vec<u8>, ExportError> {
        let mut wtr = csv::Writer::from_writer(vec![]);
        wtr.write_record(["商品ID", "商品名", "类型", "单价", "数量", "原价合计", "已成盒数", "未成盒数", "赠品数量", "备注"])?;

        for item in &snapshot.item_totals {
            wtr.write_record([
                item.item_id.0.as_str(),
                item.item_name.as_str(),
                item.kind.as_str(),
                &format_money(item.unit_price),
                &item.total_quantity.to_string(),
                &format_money(item.gross_total),
                &item.box_count.to_string(),
                &item.incomplete_box_count.to_string(),
                &item.gift_quantity.to_string(),
                item.notes.as_deref().unwrap_or(""),
            ])?;
        }

        wtr.flush()?;
        Ok(wtr.into_inner().map_err(|e| ExportError::Io(e.into_error()))?)
    }

    pub fn export_order_helper(&self, snapshot: &SettlementSnapshot) -> Result<Vec<u8>, ExportError> {
        let mut wtr = csv::Writer::from_writer(vec![]);
        wtr.write_record(["商品名", "购买数量", "平台下单单价", "总额", "参与优惠范围", "备注"])?;

        for item in &snapshot.item_totals {
            wtr.write_record([
                item.item_name.as_str(),
                &item.total_quantity.to_string(),
                &format_money(item.unit_price),
                &format_money(item.gross_total),
                if item.kind == "split" || item.kind == "single" { "是" } else { "否" },
                item.notes.as_deref().unwrap_or(""),
            ])?;
        }

        wtr.flush()?;
        Ok(wtr.into_inner().map_err(|e| ExportError::Io(e.into_error()))?)
    }
}
