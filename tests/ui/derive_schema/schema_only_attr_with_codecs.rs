use rustts::TsSchema;

#[derive(TsSchema)]
struct Invoice {
    #[rustts(rename = "invoiceId")]
    invoice_id: String,
}

fn main() {}
