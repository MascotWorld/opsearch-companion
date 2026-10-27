// Package vdf parses Steam text and binary KeyValues1 files.
package vdf

import (
	"bytes"
	"encoding/binary"
	"errors"
	"fmt"
	"math"
	"strconv"
	"strings"
	"unicode/utf16"
	"unicode/utf8"
)

// Node is an ordered VDF object. Steam can repeat keys; preserving order avoids
// silently discarding information while callers inspect the tree.
type Node struct{ Pairs []Pair }
type Pair struct {
	Key   string
	Value Value
}
type Value struct {
	Text   string
	Object *Node
}

func Decode(data []byte) (*Node, error) {
	if len(data) > 8<<20 {
		return nil, errors.New("VDF exceeds the 8 MiB parser limit")
	}
	if len(data) > 0 && data[0] == 0 {
		return decodeBinary(data)
	}
	data = bytes.TrimPrefix(data, []byte{0xef, 0xbb, 0xbf})
	if len(data) >= 2 && ((data[0] == 0xff && data[1] == 0xfe) || (data[0] == 0xfe && data[1] == 0xff)) {
		le := data[0] == 0xff
		u := make([]uint16, (len(data)-2)/2)
		for i := range u {
			a, b := data[2+i*2], data[3+i*2]
			if le {
				u[i] = uint16(a) | uint16(b)<<8
			} else {
				u[i] = uint16(a)<<8 | uint16(b)
			}
		}
		data = []byte(string(utf16.Decode(u)))
	}
	if !utf8.Valid(data) {
		return nil, errors.New("VDF text is not valid UTF-8 or UTF-16")
	}
	t := tokenizer{src: string(data)}
	root := &Node{}
	if err := t.object(root, false); err != nil {
		return nil, err
	}
	if _, err := t.next(); err == nil {
		return nil, errors.New("trailing tokens after root object")
	} else if !errors.Is(err, errEOF) {
		return nil, err
	}
	return root, nil
}

// decodeBinary handles Steam's ordinary binary KeyValues1 encoding (not
// appinfo.vdf's string-table extension or Source 2 VBKV).
func decodeBinary(data []byte) (*Node, error) {
	p := binaryParser{b: data}
	typ, rootName, e := p.header()
	if e != nil {
		return nil, e
	}
	if typ != 0 {
		return nil, fmt.Errorf("binary VDF root must be an object, got type %d", typ)
	}
	root := &Node{}
	child := &Node{}
	if e = p.object(child, 1); e != nil {
		return nil, e
	}
	// Steam's root node has its ordinary object terminator and a final
	// top-level end marker (so normal files finish with 08 08 after the last
	// child object terminator).
	if p.i >= len(data) || data[p.i] != 8 {
		return nil, errors.New("binary VDF is missing its final root marker")
	}
	p.i++
	root.Pairs = append(root.Pairs, Pair{rootName, Value{Object: child}})
	if p.i != len(data) {
		return nil, fmt.Errorf("binary VDF has %d trailing bytes", len(data)-p.i)
	}
	return root, nil
}

type binaryParser struct {
	b []byte
	i int
}

func (p *binaryParser) header() (byte, string, error) {
	if p.i >= len(p.b) {
		return 0, "", errors.New("empty binary VDF")
	}
	t := p.b[p.i]
	p.i++
	k, e := p.cstring()
	return t, k, e
}
func (p *binaryParser) cstring() (string, error) {
	start := p.i
	for p.i < len(p.b) && p.b[p.i] != 0 {
		p.i++
	}
	if p.i == len(p.b) {
		return "", errors.New("unterminated binary VDF string")
	}
	s := string(p.b[start:p.i])
	p.i++
	return s, nil
}
func (p *binaryParser) take(n int) ([]byte, error) {
	if n < 0 || p.i+n > len(p.b) {
		return nil, errors.New("truncated binary VDF value")
	}
	v := p.b[p.i : p.i+n]
	p.i += n
	return v, nil
}
func (p *binaryParser) object(n *Node, depth int) error {
	if depth > 128 {
		return errors.New("binary VDF nesting exceeds 128 levels")
	}
	for {
		if p.i >= len(p.b) {
			return errors.New("binary VDF object has no end marker")
		}
		t := p.b[p.i]
		p.i++
		if t == 8 {
			return nil
		}
		if t == 11 {
			return errors.New("unsupported Source 2 alternate end marker")
		}
		key, e := p.cstring()
		if e != nil {
			return e
		}
		v := Value{}
		switch t {
		case 0:
			c := &Node{}
			if e = p.object(c, depth+1); e != nil {
				return e
			}
			v.Object = c
		case 1:
			v.Text, e = p.cstring()
		case 2:
			var b []byte
			b, e = p.take(4)
			if e == nil {
				v.Text = strconv.FormatInt(int64(int32(binary.LittleEndian.Uint32(b))), 10)
			}
		case 3:
			var b []byte
			b, e = p.take(4)
			if e == nil {
				v.Text = strconv.FormatFloat(float64(math.Float32frombits(binary.LittleEndian.Uint32(b))), 'g', -1, 32)
			}
		case 4:
			var b []byte
			b, e = p.take(4)
			if e == nil {
				v.Text = strconv.FormatUint(uint64(binary.LittleEndian.Uint32(b)), 10)
			}
		case 5: // Steam's NUL-terminated UTF-16LE string form.
			start := p.i
			for p.i+1 < len(p.b) && (p.b[p.i] != 0 || p.b[p.i+1] != 0) {
				p.i += 2
			}
			if p.i+1 >= len(p.b) {
				e = errors.New("unterminated binary VDF wide string")
			} else {
				u := make([]uint16, (p.i-start)/2)
				for i := range u {
					u[i] = binary.LittleEndian.Uint16(p.b[start+i*2:])
				}
				v.Text = string(utf16.Decode(u))
				p.i += 2
			}
		case 6:
			var b []byte
			b, e = p.take(4)
			if e == nil {
				v.Text = strconv.FormatUint(uint64(binary.LittleEndian.Uint32(b)), 10)
			}
		case 7:
			var b []byte
			b, e = p.take(8)
			if e == nil {
				v.Text = strconv.FormatUint(binary.LittleEndian.Uint64(b), 10)
			}
		case 9:
			var b []byte
			b, e = p.take(4)
			if e == nil {
				size := binary.LittleEndian.Uint32(b)
				if size > 1<<20 {
					e = errors.New("binary VDF blob exceeds 1 MiB")
				} else {
					var blob []byte
					blob, e = p.take(int(size))
					v.Text = fmt.Sprintf("%x", blob)
				}
			}
		case 10:
			var b []byte
			b, e = p.take(8)
			if e == nil {
				v.Text = strconv.FormatInt(int64(binary.LittleEndian.Uint64(b)), 10)
			}
		default:
			return fmt.Errorf("unsupported binary VDF type %d at byte %d", t, p.i-1)
		}
		if e != nil {
			return fmt.Errorf("binary VDF key %q: %w", key, e)
		}
		n.Pairs = append(n.Pairs, Pair{key, v})
	}
}

func (n *Node) Get(key string) (Value, bool) {
	for _, p := range n.Pairs {
		if strings.EqualFold(p.Key, key) {
			return p.Value, true
		}
	}
	return Value{}, false
}

var errEOF = errors.New("end of file")

type tokenizer struct {
	src string
	i   int
}

func (t *tokenizer) next() (string, error) {
	for t.i < len(t.src) {
		c := t.src[t.i]
		if c == ' ' || c == '\t' || c == '\r' || c == '\n' || c == ';' {
			t.i++
			continue
		}
		if c == '/' && t.i+1 < len(t.src) && t.src[t.i+1] == '/' {
			for t.i < len(t.src) && t.src[t.i] != '\n' {
				t.i++
			}
			continue
		}
		break
	}
	if t.i >= len(t.src) {
		return "", errEOF
	}
	if t.src[t.i] == '{' || t.src[t.i] == '}' {
		c := t.src[t.i]
		t.i++
		return string(c), nil
	}
	if t.src[t.i] == '"' {
		t.i++
		var b strings.Builder
		for t.i < len(t.src) {
			c := t.src[t.i]
			t.i++
			if c == '"' {
				return b.String(), nil
			}
			if c == '\\' {
				if t.i >= len(t.src) {
					return "", errors.New("unterminated escape")
				}
				e := t.src[t.i]
				t.i++
				switch e {
				case 'n':
					b.WriteByte('\n')
				case 'r':
					b.WriteByte('\r')
				case 't':
					b.WriteByte('\t')
				case '\\', '"':
					b.WriteByte(e)
				default:
					b.WriteByte('\\')
					b.WriteByte(e)
				}
			} else {
				b.WriteByte(c)
			}
		}
		return "", errors.New("unterminated quoted string")
	}
	start := t.i
	for t.i < len(t.src) {
		c := t.src[t.i]
		if c == ' ' || c == '\t' || c == '\r' || c == '\n' || c == '{' || c == '}' {
			break
		}
		t.i++
	}
	if start == t.i {
		return "", fmt.Errorf("unexpected character %q at byte %d", t.src[t.i], t.i)
	}
	return t.src[start:t.i], nil
}
func (t *tokenizer) object(n *Node, nested bool) error {
	for {
		k, err := t.next()
		if err != nil {
			if errors.Is(err, errEOF) && !nested {
				return nil
			}
			if errors.Is(err, errEOF) {
				return errors.New("missing closing brace")
			}
			return err
		}
		if k == "}" {
			if !nested {
				return errors.New("unexpected closing brace")
			}
			return nil
		}
		if k == "{" {
			return errors.New("expected key before opening brace")
		}
		v, err := t.next()
		if err != nil {
			return fmt.Errorf("key %q has no value: %w", k, err)
		}
		if v == "{" {
			child := &Node{}
			if err = t.object(child, true); err != nil {
				return err
			}
			n.Pairs = append(n.Pairs, Pair{k, Value{Object: child}})
		} else if v == "}" {
			return fmt.Errorf("key %q has no value", k)
		} else {
			n.Pairs = append(n.Pairs, Pair{k, Value{Text: v}})
		}
	}
}
