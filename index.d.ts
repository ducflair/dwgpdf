export type PageSize =
  | "Auto"
  | "A0"
  | "A1"
  | "A2"
  | "A3"
  | "A4"
  | "A5"
  | "Letter"
  | "Legal"
  | "Tabloid"
  | { Custom: [widthPt: number, heightPt: number] };

export type Orientation = "Auto" | "Portrait" | "Landscape";

export type ColorMode = "Normal" | "Monochrome" | "Grayscale" | "Invert";

export type ConversionMode =
  | "Plot"
  | {
      Crop: {
        offset_x: number;
        offset_y: number;
        width?: number | null;
        height?: number | null;
      };
    };

export interface Metadata {
  title?: string | null;
  author?: string | null;
  subject?: string | null;
  creator?: string | null;
}

export interface ConversionOptions {
  page_size?: PageSize;
  orientation?: Orientation;
  mode?: ConversionMode;
  color_mode?: ColorMode;
  margin_pt?: number;
  scale?: number | null;
  use_lineweights?: boolean;
  line_width_scale?: number;
  min_line_width_pt?: number;
  background_color?: string | null;
  default_draw_color?: string | null;
  export_layers?: boolean;
  layout?: string | null;
  metadata?: Metadata;
}

export function convert(input: Uint8Array): Uint8Array;

export function convertWithOptions(
  input: Uint8Array,
  options: ConversionOptions,
): Uint8Array;
