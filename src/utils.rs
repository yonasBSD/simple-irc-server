// utils.rs - commands
//
// simple-irc-server - simple IRC server
// Copyright (C) 2022  Mateusz Szpakowski
//
// This library is free software; you can redistribute it and/or
// modify it under the terms of the GNU Lesser General Public
// License as published by the Free Software Foundation; either
// version 2.1 of the License, or (at your option) any later version.
//
// This library is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// Lesser General Public License for more details.
//
// You should have received a copy of the GNU Lesser General Public
// License along with this library; if not, write to the Free Software
// Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA  02110-1301  USA

use std::error::Error;
use bytes::{BufMut, BytesMut};
use tokio_util::codec::{Framed, LinesCodec, LinesCodecError, Decoder, Encoder};
use validator::ValidationError;

use crate::command::CommandId::*;
use crate::command::CommandError;
use crate::command::CommandError::*;

// special LinesCodec for IRC - encode with "\r\n".
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(crate) struct IRCLinesCodec(LinesCodec);

impl IRCLinesCodec {
    pub fn new() -> IRCLinesCodec {
        IRCLinesCodec(LinesCodec::new())
    }
}

impl Encoder<String> for IRCLinesCodec {
    type Error = LinesCodecError;

    fn encode(&mut self, line: String, buf: &mut BytesMut) -> Result<(), Self::Error> {
        buf.reserve(line.len() + 1);
        buf.put(line.as_bytes());
        // put "\r\n"
        buf.put_u8(b'\r');
        buf.put_u8(b'\n');
        Ok(())
    }
}

impl Decoder for IRCLinesCodec {
    type Item = String;
    type Error = LinesCodecError;
    
    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<String>, Self::Error> {
        self.0.decode(buf)
    }
}

pub fn validate_username(username: &str) -> Result<(), ValidationError> {
    if username.len() != 0 && (username.as_bytes()[0] == b'#' ||
            username.as_bytes()[0] == b'&') {
        Err(ValidationError::new("Username must not have channel prefix."))
    } else if !username.contains('.') && !username.contains(':') && !username.contains(',') {
        Ok(())
    } else {
        Err(ValidationError::new("Username must not contains '.', ',' or ':'."))
    }
}

pub fn validate_channel(channel: &str) -> Result<(), ValidationError> {
    if channel.len() != 0 && !channel.contains(':') && !channel.contains(',') &&
        (channel.as_bytes()[0] == b'#' || channel.as_bytes()[0] == b'&') {
        Ok(())
    } else {
        Err(ValidationError::new("Channel name must have '#' or '&' at start and \
                must not contains ',' or ':'."))
    }
}

pub(crate) fn validate_server<E: Error>(s: &str, e: E) -> Result<(), E> {
    if s.contains('.') { Ok(()) }
    else { Err(e) }
}

pub(crate) fn validate_server_mask<E: Error>(s: &str, e: E) -> Result<(), E>  {
    if s.contains('.') | s.contains('*') { Ok(()) }
    else { Err(e) }
}

pub(crate) fn validate_prefixed_channel<E: Error>(channel: &str, e: E) -> Result<(), E> {
    if channel.len() != 0 && !channel.contains(':') && !channel.contains(',') {
        let cfirst = channel.as_bytes()[0];
        if channel.len() >= 3 && cfirst==b'&' && (
            channel.as_bytes()[1] == b'&' || channel.as_bytes()[1] == b'#') {
            return Ok(());  // protected prefix
        }
        let channel = if cfirst == b'~' || cfirst == b'@' || cfirst == b'%' ||
                    cfirst == b'+' {
            &channel[1..]
        } else { channel };
        let cfirst = channel.as_bytes()[0];
        if cfirst == b'#' || cfirst == b'&' {
            Ok(())
        } else { Err(e) }
    } else { Err(e) }
}

pub(crate) fn validate_usermodes<'a>(modes: &Vec<(&'a str, Vec<&'a str>)>)
                -> Result<(), CommandError> {
    let mut param_idx = 1;
    modes.iter().try_for_each(|(ms, margs)| {
        if ms.len() != 0 {
            if ms.find(|c|
                c!='+' && c!='-' && c!='i' && c!='o' &&
                    c!='O' && c!='r' && c!='w').is_some() {
                Err(UnknownUModeFlag(param_idx))
            } else if margs.len() != 0 {
                Err(WrongParameter(MODEId, param_idx))
            } else {
                param_idx += 1;
                Ok(())
            }
        } else { // if empty
            Err(WrongParameter(MODEId, param_idx))
        }
    })
}

pub(crate) fn validate_channelmodes<'a>(target: &'a str, modes: &Vec<(&'a str, Vec<&'a str>)>)
                -> Result<(), CommandError> {
    let mut param_idx = 1;
    modes.iter().try_for_each(|(ms, margs)| {
        if ms.len() != 0 {
            let mut mode_set = false;
            let mut arg_param_idx = param_idx+1;
            
            let mut margs_it = margs.iter();
            
            ms.chars().try_for_each(|c| {
                match c {
                    '+' => { mode_set = true; }
                    '-' => { mode_set = false; }
                    'b'|'e'|'I' => {
                        margs_it.next(); // consume argument
                        arg_param_idx += 1;
                    }
                    'o'|'v'|'h' => {
                        if let Some(arg) = margs_it.next() {
                            validate_username(arg).map_err(|e|
                                InvalidModeParam{ target: target.to_string(),
                                        modechar: c, param: arg.to_string(),
                                        description: e.to_string() })?;
                            arg_param_idx += 1;
                        } else {
                            return Err(InvalidModeParam{ target: target.to_string(),
                                            modechar: c, param: "".to_string(),
                                            description: "No argument".to_string() });
                        }
                    }
                    'l' => {
                        if mode_set {
                            if let Some(arg) = margs_it.next() {
                                if let Err(e) = arg.parse::<usize>() {
                                    return Err(InvalidModeParam{ target: target.to_string(),
                                            modechar: c, param: arg.to_string(),
                                            description: e.to_string() });
                                }
                                arg_param_idx += 1;
                            } else {
                                return Err(InvalidModeParam{ target: target.to_string(),
                                            modechar: c, param: "".to_string(),
                                            description: "No argument".to_string() });
                            }
                        } else if let Some(arg) = margs_it.next() {
                            return Err(InvalidModeParam{ target: target.to_string(),
                                        modechar: c, param: arg.to_string(),
                                        description: "Unexpected argument".to_string() });
                        }
                    }
                    'k' => {
                        if mode_set {
                            if let Some(arg) = margs_it.next() {
                                arg_param_idx += 1;
                            } else {
                                return Err(InvalidModeParam{ target: target.to_string(),
                                            modechar: c, param: "".to_string(),
                                            description: "No argument".to_string() });
                            }
                        } else if let Some(arg) = margs_it.next() {
                            return Err(InvalidModeParam{ target: target.to_string(),
                                        modechar: c, param: arg.to_string(),
                                        description: "Unexpected argument".to_string() });
                        }
                    }
                    'i'|'m'|'t'|'n'|'s'|'p' => { },
                    c => { return Err(UnknownMode(param_idx, c)); }
                }
                Ok(())
            })?;
            
            param_idx += margs.len() + 1;
            
            Ok(())
        } else { // if empty
            Err(WrongParameter(MODEId, param_idx))
        }
    })
}

fn starts_single_wilcards<'a>(pattern: &'a str, text: &'a str) -> bool {
    if pattern.len() <= text.len() {
        pattern.bytes().enumerate().all(|(i,c)| {
            c == b'?' || c == text.as_bytes()[i]
        })
    } else { false }
}

pub(crate) fn match_wildcard<'a>(pattern: &'a str, text: &'a str) -> bool {
    let mut pat = pattern;
    let mut t = text;
    let mut asterisk = false;
    while pat.len()!=0 {
        let (newpat, m) = if let Some(i) = pat.find('*') {
            (&pat[i+1..], &pat[..i])
        } else {
            (&pat[pat.len()..pat.len()], pat)
        };
        
        if m.len() != 0 {
            if !asterisk {
                // if first match
                if !starts_single_wilcards(m, t) { return false; }
                t = &t[m.len()..];
            } else {
                // after asterisk
                let mut i = 0;
                // find first single wildcards occurrence.
                while i < t.len()-m.len() && !starts_single_wilcards(m, &t[i..]) {
                    i += 1; }
                if i < t.len()-m.len() { // if found
                    t = &t[i+m.len()..]
                } else { return false; }
            }
        }
        
        asterisk = true;
        pat = newpat;
    }
    t.len() == 0
}

#[cfg(test)]
mod test {
    use super::*;
    
    #[test]
    fn test_irc_lines_codec() {
        let mut codec = IRCLinesCodec::new();
        let mut buf = BytesMut::new();
        codec.encode("my line".to_string(), &mut buf).unwrap();
        assert_eq!("my line\r\n".as_bytes(), buf);
        let mut buf = BytesMut::from("my line 2\n");
        assert_eq!(codec.decode(&mut buf).map_err(|e| e.to_string()),
                Ok(Some("my line 2".to_string())));
        assert_eq!(buf, BytesMut::new());
        let mut buf = BytesMut::from("my line 2\r\n");
        assert_eq!(codec.decode(&mut buf).map_err(|e| e.to_string()),
                Ok(Some("my line 2".to_string())));
        assert_eq!(buf, BytesMut::new());
    }
    
    #[test]
    fn test_validate_username() {
        assert_eq!(true, validate_username("ala").is_ok());
        assert_eq!(false, validate_username("#ala").is_ok());
        assert_eq!(false, validate_username("&ala").is_ok());
        assert_eq!(false, validate_username("a.la").is_ok());
        assert_eq!(false, validate_username("a,la").is_ok());
        assert_eq!(false, validate_username("aL:a").is_ok());
    }
    
    #[test]
    fn test_validate_channel() {
        assert_eq!(true, validate_channel("#ala").is_ok());
        assert_eq!(true, validate_channel("&ala").is_ok());
        assert_eq!(false, validate_channel("&al:a").is_ok());
        assert_eq!(false, validate_channel("&al,a").is_ok());
        assert_eq!(false, validate_channel("#al:a").is_ok());
        assert_eq!(false, validate_channel("#al,a").is_ok());
        assert_eq!(false, validate_channel("ala").is_ok());
    }
    
    #[test]
    fn test_validate_server() {
        assert_eq!(true, validate_server("somebody.org",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(false, validate_server("somebodyorg",
                WrongParameter(PINGId, 0)).is_ok());
    }
    
    #[test]
    fn test_validate_server_mask() {
        assert_eq!(true, validate_server_mask("somebody.org",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(true, validate_server_mask("*org",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(false, validate_server_mask("somebodyorg",
                WrongParameter(PINGId, 0)).is_ok());
    }
    
    #[test]
    fn test_validate_prefixed_channel() {
        assert_eq!(true, validate_prefixed_channel("#ala",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(true, validate_prefixed_channel("&ala",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(false, validate_prefixed_channel("&al:a",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(false, validate_prefixed_channel("&al,a",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(false, validate_prefixed_channel("#al:a",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(false, validate_prefixed_channel("#al,a",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(false, validate_prefixed_channel("ala",
                WrongParameter(PINGId, 0)).is_ok());
        
        assert_eq!(true, validate_prefixed_channel("~#ala",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(true, validate_prefixed_channel("+#ala",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(true, validate_prefixed_channel("%#ala",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(true, validate_prefixed_channel("&#ala",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(true, validate_prefixed_channel("@#ala",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(true, validate_prefixed_channel("~&ala",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(true, validate_prefixed_channel("+&ala",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(true, validate_prefixed_channel("%&ala",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(true, validate_prefixed_channel("&&ala",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(true, validate_prefixed_channel("@&ala",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(false, validate_prefixed_channel("*#ala",
                WrongParameter(PINGId, 0)).is_ok());
        assert_eq!(false, validate_prefixed_channel("*&ala",
                WrongParameter(PINGId, 0)).is_ok());
    }
    
    #[test]
    fn test_validate_usermodes() {
        assert_eq!(Ok(()), validate_usermodes(&vec![
            ("+io-rw", vec![]), ("-O", vec![])]).map_err(|e| e.to_string()));
        assert_eq!(Ok(()), validate_usermodes(&vec![
            ("+io", vec![]), ("-rO", vec![]), ("-w", vec![])])
                .map_err(|e| e.to_string()));
        assert_eq!(Err("Wrong parameter 1 in command 'MODE'".to_string()),
            validate_usermodes(&vec![("+io-rw", vec!["xx"]),
                    ("-O", vec![])]).map_err(|e| e.to_string()));
        assert_eq!(Err("Unknown umode flag in parameter 2".to_string()),
            validate_usermodes(&vec![
                ("+io-rw", vec![]), ("-x", vec![])]).map_err(|e| e.to_string()));
    }
    
    #[test]
    fn test_validate_channelmodes() {
        assert_eq!(Ok(()), validate_channelmodes("#xchan", &vec![
            ("+ntp", vec![]), ("-sm", vec![])]).map_err(|e| e.to_string()));
        assert_eq!(Ok(()), validate_channelmodes("#xchan", &vec![
            ("+nltp", vec!["22"]), ("-s+km", vec!["xxyy"])])
                .map_err(|e| e.to_string()));
        assert_eq!(Ok(()), validate_channelmodes("#xchan", &vec![
            ("+ibl-h", vec!["*dudu.com", "22", "derek"])])
                .map_err(|e| e.to_string()));
        assert_eq!(Ok(()), validate_channelmodes("#xchan", &vec![
            ("-nltp", vec![]), ("+s-km", vec![])]).map_err(|e| e.to_string()));
        assert_eq!(Ok(()), validate_channelmodes("#xchan", &vec![
            ("+ot", vec!["barry"]), ("-nh", vec!["guru"]), ("+vm", vec!["jerry"])])
                .map_err(|e| e.to_string()));
        assert_eq!(Ok(()), validate_channelmodes("#xchan", &vec![
            ("-to", vec!["barry"]), ("+hn", vec!["guru"]), ("-mv", vec!["jerry"])])
                .map_err(|e| e.to_string()));
        assert_eq!(Ok(()), validate_channelmodes("#xchan", &vec![
            ("-tb", vec!["barry"]), ("+iI", vec!["guru"]), ("-es", vec!["eagle"])])
                .map_err(|e| e.to_string()));
        assert_eq!(Ok(()), validate_channelmodes("#xchan", &vec![
            ("+tb", vec!["barry"]), ("-iI", vec!["guru"]), ("+es", vec!["eagle"])])
                .map_err(|e| e.to_string()));
        assert_eq!(Err("Unknown mode u in parameter 2".to_string()),
            validate_channelmodes("#xchan", &vec![("+ntp", vec![]), ("-sum", vec![])])
                .map_err(|e| e.to_string()));
        assert_eq!(Err("Invalid mode parameter: #xchan l  No argument".to_string()),
            validate_channelmodes("#xchan", &vec![("+nltp", vec![]), ("-s+km", vec!["xxyy"])])
                .map_err(|e| e.to_string()));
        assert_eq!(Err("Invalid mode parameter: #xchan v jer:ry Validation error: Username \
                must not contains '.', ',' or ':'. [{}]".to_string()),
            validate_channelmodes("#xchan", &vec![
                ("+ot", vec!["barry"]), ("-nh", vec!["guru"]), ("+vm", vec!["jer:ry"])])
                    .map_err(|e| e.to_string()));
        assert_eq!(Err("Invalid mode parameter: #xchan h gu:ru Validation error: Username \
                must not contains '.', ',' or ':'. [{}]".to_string()),
            validate_channelmodes("#xchan", &vec![
                ("+ot", vec!["barry"]), ("-nh", vec!["gu:ru"]), ("+vm", vec!["jerry"])])
                    .map_err(|e| e.to_string()));
        assert_eq!(Err("Invalid mode parameter: #xchan o b,arry Validation error: Username \
                must not contains '.', ',' or ':'. [{}]".to_string()),
            validate_channelmodes("#xchan", &vec![
                ("+ot", vec!["b,arry"]), ("-nh", vec!["guru"]), ("+vm", vec!["jerry"])])
                    .map_err(|e| e.to_string()));
    }
    
    #[test]
    fn test_match_wildcard() {
        assert!(match_wildcard("somebody", "somebody"));
        assert!(!match_wildcard("somebody", "somebady"));
        assert!(match_wildcard("s?meb?dy", "samebady"));
        assert!(!match_wildcard("somebody", "somebod"));
        assert!(!match_wildcard("somebody", "somebodyis"));;
    }
}
