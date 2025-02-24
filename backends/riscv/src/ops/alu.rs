use crate::utils::RTypeInstr;
use crate::{assemble_reg, disassemble_gpr};
use crate::{register_parser, Register};
use tir_backend::parser::AsmStream;
use tir_backend::BinaryEmittable;
use tir_core::parser::{AsmPResult, Parsable};
use tir_core::OpAssembly;
use tir_core::*;
use tir_macros::{Op, OpAssembly};
use winnow::ascii::space0;
use winnow::combinator::{alt, cut_err};
use winnow::Parser;

use crate::DIALECT_NAME;

const ALU_OPCODE: u8 = 0b110011;

macro_rules! alu_op_base {
    ($struct_name:ident, $op_name:literal) => {
        #[derive(Op, OpAssembly)]
        #[operation(name = $op_name)]
        pub struct $struct_name {
            #[operand]
            rs1: Register,
            #[operand]
            rs2: Register,
            #[operand]
            rd: Register,
            r#impl: OpImpl,
        }
    };
}

macro_rules! alu_ops {
    // R-format ALU operations
    ($($struct_name:ident => { name = $op_name:literal, funct7 = $funct7:literal, funct3 = $funct3:literal })*) => {
        $(
        alu_op_base!($struct_name, $op_name);
        )*

        $(
        impl BinaryEmittable for $struct_name {
            fn encode(
                &self,
                _target_opts: &tir_backend::TargetOptions,
                stream: &mut Box<dyn tir_backend::BinaryStream>,
            ) -> tir_core::Result<()> {
                let instr = RTypeInstr::builder()
                    .opcode(ALU_OPCODE)
                    .rd(assemble_reg(self.get_rd())?)
                    .funct3($funct3)
                    .rs1(assemble_reg(self.get_rs1())?)
                    .rs2(assemble_reg(self.get_rs2())?)
                    .funct7($funct7)
                    .build();
                stream.write(&instr.to_bytes());
                Ok(())
            }
        }
        )*

        pub fn disassemble_alu_instr(context: &ContextRef, stream: &[u8]) -> Option<OpRef> {
            if stream.len() < 4 {
                return None;
            }

            let instr = RTypeInstr::from_bytes(&stream[0..4].try_into().unwrap());
            if instr.opcode() != ALU_OPCODE {
                return None;
            }

            let rd = disassemble_gpr(instr.rd())?;
            let rs1 = disassemble_gpr(instr.rs1())?;
            let rs2 = disassemble_gpr(instr.rs2())?;

            match (instr.funct3(), instr.funct7()) {
                $(
                ($funct3, $funct7) => {
                    let op = $struct_name::builder(&context).rs1(rs1).rs2(rs2).rd(rd).build();
                    Some(op)
                },
                )*
                _ => None,
            }
        }

        // FIXME: find a way to support upper case syntax
        pub(crate) fn r_instr(input: &mut AsmStream<'_>) -> AsmPResult<()> {
            let mnemonic = alt([
            $(
                $op_name
            ),*
            ]);
            let comma1 = (space0, ',', space0).void();
            let comma2 = (space0, ',', space0).void();

            let builder = input.state.get_builder();
            let context = builder.get_context();

            (
                mnemonic,
                space0.void(),
                register_parser,
                cut_err(comma1),
                register_parser,
                cut_err(comma2),
                register_parser,
            )
                .verify_map(|(instr, _, rs1, _, rs2, _, rd): (&str, _, _, _, _, _, _)| match instr.to_lowercase().as_str() {
                    $(
                    $op_name => {
                        let op = $struct_name::builder(&context).rs1(rs1).rs2(rs2).rd(rd).build();
                        builder.insert(&op);
                        Some(())

                    },
                    )*
                    _ => None,
                })
                .parse_next(input)
        }
    };
}

// FIXME: all popular CPUs (x86, arm, risc-v) use little-endian. What happens if this code is
// compiled on a big-endian host?
alu_ops! {
    // For the sake of parser stability, temporarily put sltu before any other operation
    SltuOp => { name = "sltu", funct7 = 0b0000000, funct3 = 0b011 }
    AddOp => { name = "add", funct7 = 0b0000000, funct3 = 0b000 }
    SubOp => { name = "sub", funct7 = 0b0100000, funct3 = 0b000 }
    SllOp => { name = "sll", funct7 = 0b0000000, funct3 = 0b001 }
    SltOp => { name = "slt", funct7 = 0b0000000, funct3 = 0b010 }
    // SLTU belongs here
    SrlOp => { name = "srl", funct7 = 0b0000000, funct3 = 0b101 }
    SraOp => { name = "sra", funct7 = 0b0100000, funct3 = 0b101 }
    OrOp => { name = "or", funct7 = 0b0000000, funct3 = 0b110 }
    AndOp => { name = "and", funct7 = 0b0000000, funct3 = 0b111 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::disassemble_alu_instr;
    use std::any::TypeId;
    use tir_backend::parser::AsmParserState;
    use tir_core::{builtin::ModuleOp, Context};

    #[test]
    fn test_alu_disassembler() {
        // add x28, x6, x7
        // sub x28, x6, x7
        // sll x28, x6, x7
        // slt x28, x6, x7
        // sltu x28, x6, x7
        // srl x28, x6, x7
        // sra x28, x6, x7
        // or x28, x6, x7
        // and x28, x6, x7
        let instructions = vec![
            0x00730e33_u32,
            0x40730e33,
            0x00731e33,
            0x00732e33,
            0x00733e33,
            0x00735e33,
            0x40735e33,
            0x00736e33,
            0x00737e33,
        ];

        let context = Context::new();
        context.add_dialect(crate::create_dialect());

        let mut ops = vec![];

        for instr in instructions {
            if let Some(op) = disassemble_alu_instr(&context, &instr.to_le_bytes()) {
                ops.push(op);
            }
        }

        assert_eq!(ops.len(), 9);
        assert_eq!(ops[0].borrow().type_id(), TypeId::of::<AddOp>());
        assert_eq!(ops[1].borrow().type_id(), TypeId::of::<SubOp>());
        assert_eq!(ops[2].borrow().type_id(), TypeId::of::<SllOp>());
        assert_eq!(ops[3].borrow().type_id(), TypeId::of::<SltOp>());
        assert_eq!(ops[4].borrow().type_id(), TypeId::of::<SltuOp>());
        assert_eq!(ops[5].borrow().type_id(), TypeId::of::<SrlOp>());
        assert_eq!(ops[6].borrow().type_id(), TypeId::of::<SraOp>());
        assert_eq!(ops[7].borrow().type_id(), TypeId::of::<OrOp>());
        assert_eq!(ops[8].borrow().type_id(), TypeId::of::<AndOp>());
    }

    #[test]
    fn test_alu_disassembler_negative() {
        // _boot:
        //   addi x28, x6, 1000
        //   jal _boot
        // some bogus instr
        let instructions = vec![0x3e830e13_u32, 0xffdff0ef, 0x7fffff3];

        let context = Context::new();
        context.add_dialect(crate::create_dialect());

        let mut ops = vec![];

        for instr in instructions {
            if let Some(op) = disassemble_alu_instr(&context, &instr.to_le_bytes()) {
                ops.push(op);
            }
        }

        assert_eq!(ops.len(), 0);
    }

    macro_rules! input {
        ($inp:literal, $builder:expr) => {
            AsmStream {
                input: $inp.into(),
                state: AsmParserState::new($builder),
            }
        };
    }

    #[test]
    fn test_asm_parser() {
        let context = Context::new();
        context.add_dialect(crate::create_dialect());
        let module = ModuleOp::builder(&context).build();
        let builder = OpBuilder::new(context.clone(), module.borrow().get_body());
        assert!(r_instr
            .parse(input!("add x28, x6, x7", builder.clone()))
            .is_ok());
        assert!(r_instr
            .parse(input!("sub X28, X6, X7", builder.clone()))
            .is_ok());
        assert!(r_instr
            .parse(input!("sll X28, X6, X7", builder.clone()))
            .is_ok());
        assert!(r_instr
            .parse(input!("slt X28, X6, X7", builder.clone()))
            .is_ok());
        assert!(r_instr
            .parse(input!("sltu X28, X6, X7", builder.clone()))
            .is_ok());
        assert!(r_instr
            .parse(input!("srl X28, X6, X7", builder.clone()))
            .is_ok());
        assert!(r_instr
            .parse(input!("sra X28, X6, X7", builder.clone()))
            .is_ok());
        assert!(r_instr
            .parse(input!("or X28, X6, X7", builder.clone()))
            .is_ok());
        assert!(r_instr
            .parse(input!("and X28, X6, X7", builder.clone()))
            .is_ok());
    }
}
