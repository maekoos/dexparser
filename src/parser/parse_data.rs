use ::std::rc::Rc;
use super::raw_types::*;
use super::result_types::*;
use super::*;

fn transform_string_id_items<'a>(data: &'a[u8], sdi: &[u32], off: usize) -> nom::IResult<&'a[u8], Vec<Rc<StringData>>> {
    let mut v = vec!();
    for offset in sdi {
        v.push(Rc::new(parse_string_data_item(&data[*offset as usize - off..])?.1));
    }
    Ok((data, v))
}

fn transform_header(raw: &RawHeader, e: nom::Endianness) -> Result<Header, ParserErr> {
    Ok(Header {
        version: String::from_utf8(raw.version.to_vec())?,
        checksum: raw.checksum.to_string(),
        signature: raw.signature,
        file_size: raw.file_size,
        endianness: e
    })
}

fn transform_prototype_id_items<'a>(data: &'a[u8], proto_ids: &[RawPrototype], sd: &[Rc<StringData>],
                                    ti: &[Rc<TypeIdentifier>], off: usize, e: nom::Endianness) -> nom::IResult<&'a[u8], Vec<Rc<Prototype>>> {
    let mut v = vec!();
    for item in proto_ids {
        let shorty = sd[item.shorty_idx as usize].clone();
        let return_type = ti[item.return_type_idx as usize].clone();

        let parameters = if item.parameters_off == 0 {
            None
        } else {
            Some(parse_type_list(&data[item.parameters_off as usize - off..], e)?.1
                .list
                .into_iter()
                .map(|idx| ti[idx as usize].clone())
                .collect())
        };

        v.push(Rc::new(Prototype {shorty, return_type, parameters} ));
    }

    Ok((data, v))
}

pub fn transform_dex_file(raw: RawDexFile, e: nom::Endianness) -> Result<DexFile, ParserErr> {

    // Offsets are given, but we only have the data blob here, so we'll need to do some math
    let off = raw.header.data_off as usize;

    let header = transform_header(&raw.header, e)?;

    let sd = transform_string_id_items(&raw.data, &raw.string_id_items, off)?.1;

    let ti = raw.type_id_items.into_iter()
        .map(|i| Rc::new(TypeIdentifier { descriptor: sd[i as usize].clone() })).collect::<Vec<_>>();

    let pro = transform_prototype_id_items(&raw.data, &raw.proto_id_items, &sd, &ti, off, header.endianness)?.1;

    let fields = raw.field_id_items.into_iter()
        .map(|i| Rc::new(Field {
            definer: ti[i.class_idx as usize].clone(),
            type_: ti[i.type_idx as usize].clone(),
            name: sd[i.name_idx as usize].clone()
        })).collect::<Vec<_>>();

    let methods = raw.method_id_items.into_iter()
        .map(|i| Rc::new(Method {
            definer: ti[i.class_idx as usize].clone(),
            prototype: pro[i.proto_idx as usize].clone(),
            name: sd[i.name_idx as usize].clone()
        })).collect::<Vec<_>>();

    let class_def_items = transform_class_defs(&raw.data, off, &raw.class_def_items, &ti, &sd,
                                               &fields, &methods, header.endianness)?.1;

    Ok(DexFile {
        header,
        string_data: sd,
        type_identifiers: ti,
        prototypes: pro,
        fields,
        methods,
        class_def_items
    })
}

fn transform_annotations<'a>(data: &'a[u8], off: usize, data_off: usize, sd: &[Rc<StringData>],
                             ti: &[Rc<TypeIdentifier>], fi: &[Rc<Field>], mi: &[Rc<Method>],
                             e: nom::Endianness) -> nom::IResult<&'a[u8], Annotations> {

    let adi = parse_annotations_directory_item(&data[off - data_off..], e)?.1;

    let class_annotations = if adi.class_annotations_off == 0 {
        None
    } else {
        let set_item = parse_annotation_set_item(&data[adi.class_annotations_off as usize - data_off..], e)?.1;

        let mut class_annotations = vec!();
        // Each entry here is an offset to an annotation_item in the data pool
        for annotation_offset in set_item.entries {
            // Every annotation item contains a visibility, a type and an annotation
            let annotation_item = parse_annotation_item(&data[annotation_offset as usize - data_off..])?.1;

            class_annotations.push(ClassAnnotation {
                visibility: annotation_item.visibility,
                type_: ti[annotation_item.annotation.type_idx as usize].clone(),
                elements: annotation_item.annotation.elements.into_iter().map(|i| {
                    AnnotationElement {
                        name: sd[i.name_idx as usize].clone(),
                        value: i.value
                    }
                }).collect()
            });
        }

        Some(class_annotations)
    };

    // TODO: break this into a function
    let field_annotations = match adi.fld_annot {
        Some(raw_field_annotations) => {
            let mut fa = vec!();
            // convert raw field annotations to sensible ones
            for rfa in raw_field_annotations {
                fa.push(FieldAnnotation {
                    field_data: fi[rfa.field_idx as usize].clone(),
                    annotations: parse_annotations(&data, rfa.annotations_offset as usize, data_off, e)?.1
                })
            }
            Some(fa)
        },
        None => None
    };

    let method_annotations = match adi.mtd_annot {
        Some(raw_method_annotations) => {
            let mut ma = vec!();
            for rma in raw_method_annotations {
                ma.push(MethodAnnotation {
                    method: mi[rma.method_idx as usize].clone(),
                    annotations: parse_annotations(&data, rma.annotations_offset as usize, data_off, e)?.1
                })
            }
            Some(ma)
        },
        None => None
    };

    let parameter_annotations = match adi.prm_annot {
        Some(raw_parameter_annotations) => {
            let mut pa = vec!();
            for rpa in raw_parameter_annotations {
                let asrl = parse_annotation_set_ref_list(&data[rpa.annotations_offset as usize - data_off..], e)?.1;

                for annot_set_offset in asrl.entries {
                    if annot_set_offset != 0 {
                        pa.push(Some(ParameterAnnotation {
                            method: mi[rpa.method_idx as usize].clone(),
                            annotations: parse_annotations(&data, annot_set_offset as usize, data_off, e)?.1
                        }))
                    }
                }
            }
            Some(pa)
        },
        None => None
    };

    Ok((data, Annotations {
        class_annotations,
        field_annotations,
        method_annotations,
        parameter_annotations
    }))
}

fn parse_annotations(data: &[u8], off: usize, data_off: usize, e: nom::Endianness) -> nom::IResult<&[u8], Vec<AnnotationItem>> {
    let mut annotations = vec!();
    let asi = parse_annotation_set_item(&data[off - data_off..], e)?.1;
    for annot_offset in asi.entries {
        annotations.push(parse_annotation_item(&data[annot_offset as usize - data_off..])?.1);
    }
    Ok((data, annotations))
}

fn transform_class_defs<'a>(data: &'a[u8], data_off: usize, cdis: &[RawClassDefinition], ti: &[Rc<TypeIdentifier>],
                            sd: &[Rc<StringData>], fi: &[Rc<Field>], mtd: &[Rc<Method>],
                            e: nom::Endianness) -> nom::IResult<&'a[u8], Vec<ClassDefinition>> {

    let mut v = Vec::with_capacity(cdis.len());

    for cdi in cdis {
        let class_type = ti[cdi.class_idx as usize].clone();

        let access_flags = AccessFlag::parse(cdi.access_flags, AnnotationType::Class);

        let superclass = if cdi.superclass_idx == NO_INDEX {
            None
        } else {
            Some(ti[cdi.superclass_idx as usize].clone())
        };

        let interfaces = if cdi.interfaces_off == 0 {
            None
        } else {
            Some(parse_type_list(&data[cdi.interfaces_off as usize ..], e)?
                .1
                .list
                .into_iter()
                .map(|idx| ti[idx as usize].clone())
                .collect())
        };

        let annotations = if cdi.annotations_off == 0 {
            None
        } else {
            Some(transform_annotations(&data, cdi.annotations_off as usize, data_off, &sd, &ti, &fi, &mtd, e)?.1)
        };

        let source_file_name = if cdi.source_file_idx == NO_INDEX {
            None
        } else {
            Some(sd[cdi.source_file_idx as usize].clone())
        };

        let class_data = if cdi.class_data_off == 0 {
            None
        } else {
            let class_data = parse_class_data_item(&data[cdi.class_data_off as usize - data_off..])?.1;

            let static_fields = transform_encoded_fields(&class_data.static_fields, &fi);
            let instance_fields = transform_encoded_fields(&class_data.instance_fields, &fi);
            let direct_methods = transform_encoded_methods(&data, data_off, &class_data.direct_methods, &mtd, &ti, e)?.1;
            let virtual_methods = transform_encoded_methods(&data, data_off, &class_data.virtual_methods, &mtd, &ti, e)?.1;

            Some(ClassData { static_fields, instance_fields, direct_methods, virtual_methods })
        };

        // TODO: static values

        v.push(ClassDefinition {
            class_type, access_flags, superclass,
            interfaces, source_file_name, annotations,
            class_data });
    }

    Ok((data, v))
}

// Encoded fields are stored sequentially, with each index in the raw encoded field being the *diff*
// of the index (not the total index) from the previous entry
fn transform_encoded_fields(raw: &[RawEncodedField], fi: &[Rc<Field>]) -> Vec<EncodedField> {
    let mut fields = vec!();
    // The first entry effectively has an offset of 0
    let mut prev_offset = 0;
    // Subsequent entry indexes are offsets of the previous entry index
    for field in raw {
        fields.push(EncodedField {
            field: fi[(prev_offset + field.field_idx_diff) as usize].clone(),
            access_flags: AccessFlag::parse(field.access_flags as u32, AnnotationType::Field)
        });
        prev_offset = field.field_idx_diff;
    }
    fields
}

fn transform_code_item<'a>(data: &'a[u8], data_off: usize, raw: RawCodeItem, ti: &[Rc<TypeIdentifier>],
e: nom::Endianness) -> nom::IResult<&'a[u8], Code> {

    let debug_info = if raw.debug_info_off == 0 {
        None
    } else {
        let rdi = parse_debug_info_item(&data[raw.debug_info_off as usize - data_off ..], e)?.1;

        Some(DebugInfo {
            line_start: rdi.line_start,
            parameter_names: rdi.parameter_names,
            bytecode: rdi.bytecode.into_iter().map(DebugItemBytecodes::parse).collect()
        })
    };

    let tries = if let Some(raw_tries) = raw.tries {
        let mut tries = Vec::with_capacity(raw_tries.len());
        for raw_try in raw_tries {

            let code_units = count!(&data[raw_try.start_addr as usize - data_off ..],
                            u16!(e), raw_try.insn_count as usize)?.1;

            let handler = {
                let rh = parse_encoded_catch_handler_list(&data[raw_try.handler_off as usize - data_off ..])?.1;
                transform_encoded_catch_handler_list(rh, &ti)
            };

            tries.push(TryItem {
                code_units,
                handler
            });
        }
        Some(tries)
    } else {
        None
    };

    let handlers = if let Some(rh) = raw.handlers {
        Some(transform_encoded_catch_handler_list(rh, &ti))
    } else {
        None
    };

    Ok((data, Code {
        registers_size: raw.registers_size,
        ins_size: raw.ins_size,
        outs_size: raw.outs_size,
        debug_info,
        insns: raw.insns,
        tries,
        handlers
    }))
}

fn transform_encoded_methods<'a>(data: &'a[u8], data_off: usize, raw: &[RawEncodedMethod],
                                 mtd: &[Rc<Method>], ti: &[Rc<TypeIdentifier>], e: nom::Endianness)
                                    -> nom::IResult<&'a[u8], Vec<EncodedMethod>> {
    let mut methods = vec!();
    let mut prev_offset = 0;
    for method in raw {
        let code = if method.code_off == 0 {
            None
        } else {
            let ci = parse_code_item(&data[method.code_off as usize - data_off ..], e)?.1;
            Some(transform_code_item(data, data_off, ci, &ti, e)?.1)
        };

        methods.push(EncodedMethod {
            method: mtd[(prev_offset + method.method_idx_diff) as usize].clone(),
            access_flags: AccessFlag::parse(method.access_flags as u32, AnnotationType::Method),
            code
        });

        prev_offset = method.method_idx_diff;
    }

    Ok((data, methods))
}

// We can destructure out the RawEncodedCatchHandlerList and just return a Vec of EncodedCatchHandler's
fn transform_encoded_catch_handler_list(rh: RawEncodedCatchHandlerList, ti: &[Rc<TypeIdentifier>]) -> Vec<EncodedCatchHandler> {
    rh.list.into_iter()
        .map(|raw| EncodedCatchHandler {
            handlers: raw.handlers.into_iter()
                .map(|raw| EncodedTypeAddrPair {
                    type_: ti[raw.type_idx as usize].clone(),
                    addr: raw.addr
                }).collect(),
            catch_all_addr: raw.catch_all_addr
        }).collect()
}

// Docs: debug_info_item
named_args!(parse_debug_info_item(e: nom::Endianness)<&[u8], RawDebugInfoItem>,
    peek!(
        do_parse!(
            line_start: call!(parse_uleb128)    >>
            parameters_size: call!(parse_uleb128)   >>
            parameter_names: count!(call!(parse_uleb128p1), parameters_size as usize)    >>
            // TODO: take one byte extra to get the end sequence
            bytecode: map!(take_until!("\0"), |i| { i.to_vec() })    >>
            (RawDebugInfoItem { line_start, parameters_size, parameter_names, bytecode })
        )
    )
);

// Docs: code_item
named_args!(parse_code_item(e: nom::Endianness)<&[u8], RawCodeItem>,
    peek!(
        do_parse!(
            registers_size: u16!(e) >>
            ins_size: u16!(e)   >>
            outs_size: u16!(e)  >>
            tries_size: u16!(e) >>
            debug_info_off: u32!(e) >>
            insns_size: u32!(e) >>
            insns: count!(u16!(e), insns_size as usize)  >>
            padding: cond!(tries_size != 0 && insns_size % 2 != 0, u16!(e))  >>
            tries: cond!(tries_size != 0, count!(call!(parse_try_item, e), tries_size as usize)) >>
            handlers: cond!(tries_size != 0, call!(parse_encoded_catch_handler_list))  >>
            (RawCodeItem { registers_size, ins_size, outs_size, tries_size, debug_info_off,
             insns_size, insns, padding, tries, handlers })
        )
    )
);

// Docs: try_item
named_args!(parse_try_item(e: nom::Endianness)<&[u8], RawTryItem>,
    do_parse!(
        start_addr: u32!(e) >>
        insn_count: u16!(e) >>
        handler_off: u16!(e)    >>
        (RawTryItem { start_addr, insn_count, handler_off })
    )
);

// Docs: encoded_catch_handler_list
named!(parse_encoded_catch_handler_list<&[u8], RawEncodedCatchHandlerList>,
    do_parse!(
        size: call!(parse_uleb128) >>
        list: count!(call!(parse_encoded_catch_handler), size as usize)  >>
        (RawEncodedCatchHandlerList { size, list })
    )
);

// Docs: encoded_catch_handler
named!(parse_encoded_catch_handler<&[u8], RawEncodedCatchHandler>,
    do_parse!(
        size: call!(parse_sleb128) >>
        handlers: count!(call!(parse_encoded_type_addr_pair), size as usize) >>
        catch_all_addr: cond!(size < 0, call!(parse_uleb128)) >>
        (RawEncodedCatchHandler { size, handlers, catch_all_addr })
    )
);

// Docs: encoded_type_addr_pair
named!(parse_encoded_type_addr_pair<&[u8], RawEncodedTypeAddrPair>,
    do_parse!(
        type_idx: call!(parse_uleb128)  >>
        addr: call!(parse_uleb128)  >>
        (RawEncodedTypeAddrPair { type_idx, addr })
    )
);

// Docs: string_data
named!(parse_string_data_item<&[u8], StringData>,
    peek!(
        do_parse!(
            utf16_size: call!(parse_uleb128)                    >>
            data: map!(
                    map_res!(
                        take_until_and_consume!("\0"), str::from_utf8),
                    str::to_string)                                                 >>
            (StringData { utf16_size, data })
    ))
);

// Docs: annotation_item
named!(parse_annotation_item<&[u8], AnnotationItem>,
    peek!(
        do_parse!(
            visibility: map_res!(call!(take_one), Visibility::parse)    >>
            annotation: call!(encoded_value::parse_encoded_annotation_item)    >>
            (AnnotationItem { visibility, annotation })
        )
    )
);

// Docs: annotation_set_ref_list
named_args!(parse_annotation_set_ref_list(e: nom::Endianness)<&[u8], RawAnnotationSetRefList>,
    peek!(
        do_parse!(
            size: u32!(e)   >>
            entries: count!(call!(parse_annotation_set_ref_item, e), size as usize)     >>
            (RawAnnotationSetRefList { size, entries })
        )
    )
);

// Docs: annotation_set_ref_item
named_args!(parse_annotation_set_ref_item(e: nom::Endianness)<&[u8], u32>, peek!(u32!(e)));

// Docs: annotation_offset_item
named_args!(parse_annotation_offset_item(e: nom::Endianness)<&[u8], u32>, peek!(u32!(e)));

// Docs: annotation_set_item
named_args!(parse_annotation_set_item(e: nom::Endianness)<&[u8], RawAnnotationSetItem>,
    peek!(
        do_parse!(
            size: u32!(e)                               >>
            entries: count!(call!(parse_annotation_offset_item, e), size as usize)     >>
            (RawAnnotationSetItem { size, entries })
        )
    )
);

// Docs: class_data_item
named!(parse_class_data_item<&[u8], RawClassDataItem>,
    peek!(
        do_parse!(
            static_fields_size: call!(parse_uleb128)    >>
            instance_fields_size: call!(parse_uleb128)    >>
            direct_methods_size: call!(parse_uleb128)    >>
            virtual_methods_size: call!(parse_uleb128)    >>
            static_fields: count!(parse_encoded_field, static_fields_size as usize)    >>
            instance_fields: count!(parse_encoded_field, instance_fields_size as usize)  >>
            direct_methods: count!(parse_encoded_method, direct_methods_size as usize)    >>
            virtual_methods: count!(parse_encoded_method, virtual_methods_size as usize) >>
            (RawClassDataItem { static_fields_size, instance_fields_size, direct_methods_size,
            virtual_methods_size, direct_methods, instance_fields, static_fields, virtual_methods })
        )

    )
);

// Docs: encoded_field
named!(parse_encoded_field<&[u8], RawEncodedField>,
    do_parse!(
        field_idx_diff: call!(parse_uleb128)    >>
        access_flags: call!(parse_uleb128)  >>
        (RawEncodedField { field_idx_diff, access_flags })
    )
);

// Docs: encoded_method
named!(parse_encoded_method<&[u8], RawEncodedMethod>,
    do_parse!(
        method_idx_diff: call!(parse_uleb128)   >>
        access_flags: call!(parse_uleb128)  >>
        code_off: call!(parse_uleb128)  >>
        (RawEncodedMethod { method_idx_diff, access_flags, code_off })
    )
);

impl DebugItemBytecodes {
    pub fn parse(value: u8) -> Self {
        match value {
            0x00 => DebugItemBytecodes::DBG_END_SEQUENCE,
            0x01 => DebugItemBytecodes::DBG_ADVANCE_PC,
            0x02 => DebugItemBytecodes::DBG_ADVANCE_LINE,
            0x03 => DebugItemBytecodes::DBG_START_LOCAL,
            0x04 => DebugItemBytecodes::DBG_START_LOCAL_EXTENDED,
            0x05 => DebugItemBytecodes::DBG_END_LOCAL,
            0x06 => DebugItemBytecodes::DBG_RESTART_LOCAL,
            0x07 => DebugItemBytecodes::DBG_SET_PROLOGUE_END,
            0x08 => DebugItemBytecodes::DBG_SET_EPILOGUE_BEGIN,
            0x09 => DebugItemBytecodes::DBG_SET_FILE,
            _ => DebugItemBytecodes::SPECIAL_OPCODE(value)
        }
    }
}