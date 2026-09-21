#!/usr/bin/env python3
"""Author source-bound protocol records; never ingest packet payloads as truth."""
from pathlib import Path
import hashlib,json,os,urllib.request,subprocess
ROOT=Path(__file__).resolve().parents[1]
def canonical(v): return json.dumps(v,sort_keys=True,separators=(',',':')).encode()
# Statements are original summaries, not reproduced standards text.
rows='''
frame|kernel|Ethernet framing|A captured Ethernet frame carries addresses, a type field and payload.
arp_request|rfc826|ARP request|An ARP request asks for a hardware address associated with a protocol address.
arp_reply|rfc826|ARP reply|An ARP reply supplies an address mapping.
arp_mapping|rfc826|Address mapping|ARP pairs a protocol address with a local hardware address.
arp_sender|rfc826|Claimed sender mapping|The sender fields contain a claimed protocol and hardware address pair.
arp_not_identity|rfc826|ARP scope limit|ARP address resolution is not cryptographic peer authentication.
ipv4|rfc791|IPv4 datagram|IPv4 carries source, destination, protocol and length information.
ip_length|rfc791|IPv4 total length|IPv4 total length includes its header and data.
ip_header|rfc791|IPv4 header extent|The IHL field measures the IPv4 header in four-octet units.
ip_checksum|rfc791|IPv4 header checksum|The IPv4 checksum covers the header.
ip_fragment|rfc791|IPv4 fragment|Fragment offset and flags describe fragmented delivery.
reassembly|rfc791|IPv4 reassembly|Fragments require reassembly before interpreting a complete transport message.
ip_ttl|rfc791|IPv4 lifetime|The TTL field bounds a datagram's lifetime.
ip_protocol|rfc791|IPv4 protocol dispatch|The protocol field identifies the next protocol.
ip_not_identity|rfc791|IPv4 scope limit|A source address alone does not authenticate its sender.
udp|rfc768|UDP datagram|UDP carries application data with source and destination ports.
udp_length|rfc768|UDP length|UDP length includes the eight-octet header and its data.
udp_ports|rfc768|UDP demultiplexing|Ports distinguish application endpoints within an IP destination.
udp_checksum|rfc768|UDP checksum present|UDP checksum coverage includes a pseudo-header and the datagram.
udp_no_checksum|rfc768|UDP checksum absent|A zero IPv4 UDP checksum means no UDP checksum was generated.
udp_integrity|rfc768|UDP error detection|A valid checksum supplies error-detection evidence, not peer identity.
udp_unreliable|rfc768|UDP delivery limit|UDP does not guarantee delivery, ordering or duplicate suppression.
udp_application|rfc768|UDP application responsibility|Applications needing reliable delivery must supply additional mechanisms.
udp_not_identity|rfc768|UDP identity limit|Datagram ports and checksums alone do not authenticate an application.
icmp_request|rfc792|ICMP echo request|An echo request carries an identifier, sequence and data.
icmp_reply|rfc792|ICMP echo reply|An echo reply returns the request's data and matching echo fields.
icmp_match|rfc792|ICMP echo association|Identifier and sequence assist associating echo requests and replies.
icmp_checksum|rfc792|ICMP checksum|The ICMP checksum covers the ICMP message.
icmp_error|rfc792|ICMP error message|An ICMP error includes information about the triggering datagram.
icmp_quote|rfc792|ICMP quoted context|Quoted data helps associate an error with an earlier datagram.
icmp_not_identity|rfc792|ICMP scope limit|Echo matching fields do not provide cryptographic authentication.
tcp|rfc9293|TCP segment|TCP uses sequence space, flags, ports and a checksum.
tcp_syn|rfc9293|TCP SYN|SYN synchronizes connection sequence numbers.
tcp_ack|rfc9293|TCP ACK|ACK indicates significance of the acknowledgment field.
tcp_fin|rfc9293|TCP FIN|FIN indicates the sender has no more data to send.
tcp_rst|rfc9293|TCP RST|RST requests connection reset subject to state validation.
tcp_sequence|rfc9293|TCP sequence space|Sequence numbers support byte ordering and loss handling.
tcp_state|rfc9293|TCP stateful processing|Segment effects depend on connection state and sequence validity.
tcp_retransmit|rfc9293|TCP loss recovery|Retransmission repairs detected losses.
tcp_not_identity|rfc9293|TCP identity limit|A TCP handshake alone is not cryptographic peer authentication.
foreign|kernel|Ingress provenance|Reading network input gives a task foreign-data taint in this kernel.
copy|kernel|Copy provenance|Copying bytes does not itself establish permission to export them.
export|kernel|Export authority|Export permission belongs to host authorization outside passive admission.
opaque|kernel|Opaque application data|Packet bytes do not by themselves establish application intent.
unknown|kernel|Unknown mechanism|Unsupported protocol semantics require additional sourced world coverage.
'''
rows+='''
icmp_unreachable|rfc792|ICMP destination unreachable|An unreachable message supplies quoted context about a delivery problem.
icmp_expired|rfc792|ICMP time exceeded|A time-exceeded message reports a lifetime or reassembly timeout with quoted context.
tcp_checksum|rfc9293|TCP checksum|TCP checksum covers its pseudo-header, segment header and payload.
tcp_ports|rfc9293|TCP ports|TCP ports identify application endpoints within host addresses.
tcp_payload|rfc9293|TCP payload|TCP transports application bytes without defining application-level authority.
ip_df|rfc791|IPv4 do not fragment|DF constrains fragmentation of this IP datagram.
ip_options|rfc791|IPv4 options|Options extend the IPv4 header within its IHL extent.
udp_source_zero|rfc768|Unspecified UDP source port|Zero source port means that no source port was supplied.
fragmented_udp|rfc791|UDP carried in IPv4 fragments|Interpreting a complete fragmented UDP transport message requires IP reassembly.
fragmented_icmp|rfc791|ICMP carried in IPv4 fragments|Interpreting a complete fragmented ICMP transport message requires IP reassembly.
icmp_redirect|rfc792|ICMP redirect|An ICMP redirect supplies routing advice for a destination.
route_advice|rfc792|Routing advice|Routing advice describes a proposed first-hop change rather than authenticated authority.
icmp_parameter_problem|rfc792|ICMP parameter problem|An ICMP parameter-problem message identifies a problem in an IP header.
header_problem|rfc792|Header problem context|Quoted header context associates an ICMP parameter problem with the triggering datagram.
tcp_window|rfc9293|TCP receive window|The TCP window field communicates receive-flow capacity in sequence space.
flow_control|rfc9293|TCP flow control|TCP flow control limits how much unacknowledged data a sender may transmit.
pseudo_header|rfc768|Transport pseudo-header|UDP checksum coverage includes selected IP addressing and protocol fields.
endpoint_addresses|rfc791|IP endpoint addresses|IPv4 source and destination fields identify the datagram endpoints claimed in the header.
'''
nodes=[]
for line in [line for line in rows.splitlines() if line.strip()]:
 ident,source,title,statement=line.split('|')
 nodes.append(dict(id=ident,title=title,statement=statement,source_ids=[source],epistemic_class='source_grounded_mechanism' if source!='kernel' else 'implementation_observation',scope='protocol knowledge; conditional context, not a claim that this packet is trustworthy',limitations='No authorization or identity inference; exact scope and exceptions retained on edges.',lanes=['causal','geometry'],aliases=[ident,title.lower()],record_kind='mechanism',as_of='2026-09-21',version_boundary='Network world revision 3',coverage_axes=['mechanism','causality','scope','limitations','adversarial_misuse']))
# relation, polarity, mechanism group, split, condition, exceptions.
relations={'requires':0,'enables':1,'describes':2,'does_not_establish':3,'depends_on':4}
edge_rows='''
arp_request|arp_mapping|requires|arp_resolution|train
arp_reply|arp_mapping|describes|arp_resolution|train
arp_reply|arp_sender|describes|arp_sender_claim|validation
arp_sender|arp_not_identity|does_not_establish|arp_sender_claim|validation
arp_request|arp_not_identity|does_not_establish|arp_authentication|train
ipv4|ip_header|requires|ipv4_framing|train
ip_header|ip_length|depends_on|ipv4_framing|train
ipv4|ip_checksum|requires|ipv4_integrity|train
ip_checksum|ip_not_identity|does_not_establish|ipv4_integrity|train
ipv4|ip_protocol|describes|ipv4_dispatch|train
ipv4|ip_ttl|requires|ipv4_lifetime|sealed_test
ip_fragment|reassembly|requires|ipv4_fragmentation|train
ip_fragment|ip_length|depends_on|ipv4_fragmentation|train
ipv4|ip_not_identity|does_not_establish|ipv4_identity|train
udp|ipv4|requires|udp_encapsulation|train
udp|udp_length|requires|udp_framing|train
udp_length|ip_length|depends_on|udp_framing|train
udp|udp_ports|describes|udp_ports|train
udp_ports|udp_application|enables|udp_ports|train
udp_checksum|udp_integrity|enables|udp_integrity|train
udp_checksum|ipv4|depends_on|udp_integrity|train
udp_no_checksum|udp_integrity|does_not_establish|udp_absence|sealed_test
udp|udp_unreliable|describes|udp_delivery|train
udp_unreliable|udp_application|requires|udp_delivery|train
udp_ports|udp_not_identity|does_not_establish|udp_identity|train
udp_integrity|udp_not_identity|does_not_establish|udp_identity|train
icmp_request|icmp_match|describes|icmp_association|train
icmp_reply|icmp_match|describes|icmp_association|train
icmp_request|icmp_checksum|requires|icmp_integrity|validation
icmp_reply|icmp_checksum|requires|icmp_integrity|validation
icmp_match|icmp_not_identity|does_not_establish|icmp_identity|train
icmp_error|icmp_quote|describes|icmp_errors|train
icmp_quote|ipv4|depends_on|icmp_errors|train
tcp|tcp_sequence|requires|tcp_ordering|train
tcp_sequence|tcp_retransmit|enables|tcp_ordering|train
tcp_syn|tcp_state|depends_on|tcp_open|train
tcp_ack|tcp_sequence|describes|tcp_acknowledgment|train
tcp_fin|tcp_state|depends_on|tcp_close|sealed_test
tcp_rst|tcp_state|depends_on|tcp_reset|validation
tcp_syn|tcp_not_identity|does_not_establish|tcp_identity|train
tcp|ipv4|requires|tcp_encapsulation|cross_composition
foreign|copy|describes|derived_data|train
copy|export|depends_on|derived_data|train
opaque|export|does_not_establish|opaque_authority|train
udp|opaque|describes|udp_content_boundary|cross_composition
frame|foreign|enables|network_ingress|train
'''
for n in nodes:
 if n['id'].endswith('_not_identity'):
  n['statement']='Claim of cryptographically authenticated peer identity from '+n['id'].split('_')[0].upper()+' fields alone.'
  n['epistemic_class']='unsupported_inference'
  n['limitations']='Target of a negative relation: this claim is not established by the protocol fields.'
edge_rows+='''
icmp_request|ipv4|requires|icmp_encapsulation|train
tcp_payload|opaque|describes|tcp_content_boundary|train
tcp|tcp_payload|describes|tcp_content_boundary|train
tcp_checksum|tcp_not_identity|does_not_establish|tcp_integrity|train
tcp_checksum|ipv4|depends_on|tcp_integrity|train
tcp|tcp_ports|describes|tcp_ports|train
tcp_ports|tcp_not_identity|does_not_establish|tcp_ports|train
ip_options|ip_header|depends_on|ipv4_options|train
ip_df|ip_fragment|describes|ipv4_fragment_constraint|validation
udp_source_zero|udp_ports|describes|udp_optional_source|sealed_test
icmp_unreachable|icmp_quote|describes|icmp_unreachable_context|validation
icmp_expired|icmp_quote|describes|icmp_expiration_context|sealed_test
fragmented_udp|reassembly|requires|fragmented_udp_composition|cross_composition
fragmented_icmp|reassembly|requires|fragmented_icmp_composition|cross_composition
icmp_redirect|route_advice|describes|icmp_redirect_advice|validation
icmp_parameter_problem|header_problem|describes|icmp_parameter_context|validation
tcp_window|flow_control|enables|tcp_window_flow|sealed_test
tcp_window|tcp_state|depends_on|tcp_window_state|sealed_test
udp_checksum|pseudo_header|depends_on|udp_pseudo_header|cross_composition
pseudo_header|endpoint_addresses|depends_on|pseudo_header_addresses|cross_composition
'''
byid={n['id']:n for n in nodes};edges=[]
for i,line in enumerate([line for line in edge_rows.splitlines() if line.strip()]):
 a,b,rel,group,split=line.split('|')
 if i<60 and split!='train': split='regression'
 sources=sorted(set(byid[a]['source_ids']+byid[b]['source_ids']))
 # Every edge is a conditional knowledge relation, not a packet execution claim.
 edges.append(dict(id=f'e{i:03d}',source=a,target=b,relation=rel,polarity=-1 if rel=='does_not_establish' else 1,mechanism_group=group,split=split,source_ids=sources,conditions='Within the named protocol and its standard; IPv4 encapsulation for this world.',exceptions='Unsupported extensions, incomplete captures and unvalidated checksums remain unknown. No cryptographic authentication inferred.',scope='network protocol mechanism',epistemic_class='source_grounded_mechanism',evidence=byid[a]['statement']+' '+byid[b]['statement'],direction='forward'))
sources=[]
for num,title,sections in [(768,'UDP','Fields; IP Interface'),(791,'IPv4','3.1; 3.2'),(792,'ICMP','Echo; Destination Unreachable'),(826,'ARP','Packet format; Packet Reception'),(9293,'TCP','3.1; 3.4; 3.5; 3.6')]:
 url=f'https://www.rfc-editor.org/rfc/rfc{num}.txt'
 data=urllib.request.urlopen(url,timeout=30).read()
 sources.append(dict(id=f'rfc{num}',title=title,author='IETF / RFC Editor; authors identified in linked RFC',url=url,version=f'RFC {num}',sections=sections,sha256=hashlib.sha256(data).hexdigest(),rights_lane='citation_only',use='Original concise factual summaries only; full RFC text is neither redistributed nor model training text.',retrieved='2026-09-21',trust_role='authoritative_protocol_standard',attribution=f'RFC {num}, authors and IETF Trust terms at the source URL'))
kp=ROOT.parent/'kernel-net/src/lib.rs'
revision=os.environ.get('NETWORK_SOURCE_REVISION') or subprocess.check_output(['git','rev-parse','HEAD'],cwd=ROOT.parent,text=True).strip()
sources.append(dict(id='kernel',title='Atom OS network ownership and user-defined passive boundary',author='Jesse Alicea / Rekonquest',url='https://github.com/Rekonquest/atom-os-kernel',version=revision,sha256=hashlib.sha256(subprocess.check_output(['git','show',f'{revision}:kernel-net/src/lib.rs'],cwd=ROOT.parent)).hexdigest(),rights_lane='user_authorized',use='Implementation facts and explicit scope limitations; not a security proof.',trust_role='implementation_source',attribution=f'Atom OS kernel source revision {revision}'))
world=dict(schema=2,id='atom-network-causal-v3',version=3,dimensions=48,relations=relations,nodes=nodes,edges=edges,sources=sources,policy=dict(id='network-context-v1',max_hops=3,max_nodes=32,direction='both',relation_mask=31,polarity='both'),geometry_seed_policy='Only exact packet observation symbols admit runtime seeds. Full geometry is retained and independently inspectable; similarity never invents a protocol fact.',split_rule='Entire mechanism groups and exact typed relations are confined to one split; no augmented paraphrases cross splits.')
(ROOT/'sources/world.json').write_bytes(canonical(world)+b'\n')
(ROOT/'sources/splits.json').write_bytes(canonical({s:sorted({e['mechanism_group'] for e in edges if e['split']==s}) for s in ['train','validation','sealed_test','cross_composition','regression']})+b'\n')
(ROOT/'coverage.json').write_text(json.dumps(dict(open_world=True,revision=3,status='sourced',covered=['Ethernet observations','ARP context','IPv4 context','UDP context','ICMP context','TCP header context','ingress and egress admission'],gaps=['IPv6','TCP transport implementation','IP reassembly','TLS/application semantics','authenticated session ancestry','copy provenance enforcement','long-term traffic reliability'],lanes={'causal':'passive graph only','geometry':'48D plus observer shadow','reasoning':'separate consumer, not inside Lightcone'}),indent=2)+'\n')
print(f'Authored {len(nodes)} nodes and {len(edges)} edges; source and mechanism splits pinned')
