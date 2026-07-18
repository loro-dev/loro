import { PostcardReader, PostcardWriter } from "./postcard";

export interface EncodedChangeBlock {
  readonly counterStart: number;
  readonly counterLength: number;
  readonly lamportStart: number;
  readonly lamportLength: number;
  readonly changeCount: number;
  readonly header: Uint8Array;
  readonly changeMetadata: Uint8Array;
  readonly containerIds: Uint8Array;
  readonly keys: Uint8Array;
  readonly positions: Uint8Array;
  readonly operations: Uint8Array;
  readonly deleteStartIds: Uint8Array;
  readonly values: Uint8Array;
}

export function decodeEncodedChangeBlock(bytes: Uint8Array): EncodedChangeBlock {
  const reader = new PostcardReader(bytes);
  const block: EncodedChangeBlock = {
    counterStart: reader.readU32(),
    counterLength: reader.readU32(),
    lamportStart: reader.readU32(),
    lamportLength: reader.readU32(),
    changeCount: reader.readU32(),
    header: reader.readBytes(),
    changeMetadata: reader.readBytes(),
    containerIds: reader.readBytes(),
    keys: reader.readBytes(),
    positions: reader.readBytes(),
    operations: reader.readBytes(),
    deleteStartIds: reader.readBytes(),
    values: reader.readBytes(),
  };
  reader.assertEnd();
  return block;
}

export function encodeEncodedChangeBlock(block: EncodedChangeBlock): Uint8Array {
  const writer = new PostcardWriter();
  writer.writeU32(block.counterStart);
  writer.writeU32(block.counterLength);
  writer.writeU32(block.lamportStart);
  writer.writeU32(block.lamportLength);
  writer.writeU32(block.changeCount);
  writer.writeBytes(block.header);
  writer.writeBytes(block.changeMetadata);
  writer.writeBytes(block.containerIds);
  writer.writeBytes(block.keys);
  writer.writeBytes(block.positions);
  writer.writeBytes(block.operations);
  writer.writeBytes(block.deleteStartIds);
  writer.writeBytes(block.values);
  return writer.toUint8Array();
}
